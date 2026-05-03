use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::scraper::fetch_with_sleep;
use crate::types::{compute_id, Paper};

// ---------------------------------------------------------------------------
// 定数
// ---------------------------------------------------------------------------

/// 1ページ最大件数（CiNii Research の仕様上限）
const PAGE_SIZE: u32 = 200;

/// `start` パラメータの上限（CiNii Research の仕様上限）
const MAX_START: u32 = 10000;

/// 採用するエンドポイント．`articles` は学術論文に絞った検索種別．
const ENDPOINT: &str = "https://cir.nii.ac.jp/opensearch/v2/articles";

/// 抄録ライセンスフラグ．`allow` 以外は再配布不可なので格納しない．
const LICENSE_ALLOW: &str = "allow";

// ---------------------------------------------------------------------------
// CiNii Research JSON-LD レスポンス型
// ---------------------------------------------------------------------------

/// JSON-LDレスポンスのラッパ．
///
/// CiNii Research は `@graph` または `items` のいずれかで結果配列を返す．
/// ローカライズ済みフィールドは形が一定でない（`String` / `{@value, @language}` / `Array`）ため
/// `serde_json::Value` で受けて後段で抽出する．
#[derive(Debug, Deserialize)]
pub(crate) struct CiniiResponse {
    #[serde(rename = "@graph", default)]
    pub(crate) graph: Vec<Value>,
    #[serde(default)]
    pub(crate) items: Vec<Value>,
}

impl CiniiResponse {
    /// `@graph` を優先し，無ければ `items` を返す
    pub(crate) fn results(&self) -> &[Value] {
        if !self.graph.is_empty() {
            &self.graph
        } else {
            &self.items
        }
    }
}

// ---------------------------------------------------------------------------
// JSON-LD ローカライズ済みフィールド抽出
// ---------------------------------------------------------------------------

/// 多言語フィールドから優先言語の文字列を取得．無ければ最初に見つかった文字列を返す．
///
/// 受け付ける形：
/// - `"plain string"`
/// - `{"@value": "...", "@language": "ja"}`
/// - `[<上記のいずれか>, ...]`（JSON-LDではcreatorが配列の入れ子になることもある）
pub(crate) fn extract_localized_text(field: Option<&Value>, prefer_lang: &str) -> Option<String> {
    fn walk(
        v: &Value,
        prefer_lang: &str,
        preferred: &mut Option<String>,
        fallback: &mut Option<String>,
    ) {
        match v {
            Value::String(s) => {
                if fallback.is_none() && !s.trim().is_empty() {
                    *fallback = Some(s.clone());
                }
            }
            Value::Object(o) => {
                if let Some(val) = o.get("@value").and_then(|v| v.as_str())
                    && !val.trim().is_empty()
                {
                    let lang = o.get("@language").and_then(|v| v.as_str()).unwrap_or("");
                    if lang == prefer_lang && preferred.is_none() {
                        *preferred = Some(val.to_string());
                    } else if fallback.is_none() {
                        *fallback = Some(val.to_string());
                    }
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    walk(item, prefer_lang, preferred, fallback);
                }
            }
            _ => {}
        }
    }

    let field = field?;
    let mut preferred = None;
    let mut fallback = None;
    walk(field, prefer_lang, &mut preferred, &mut fallback);
    preferred
        .or(fallback)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `dc:creator` から著者名のリストを抽出．
///
/// CiNii では1著者が `[{ja}, {en}]` のように複数言語表現を持つので，
/// 配列の各要素を1著者として扱い，その中から `prefer_lang` の表現を選ぶ．
pub(crate) fn extract_creators(field: Option<&Value>, prefer_lang: &str) -> Vec<String> {
    match field {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| extract_localized_text(Some(v), prefer_lang))
            .collect(),
        Some(other) => extract_localized_text(Some(other), prefer_lang)
            .into_iter()
            .collect(),
        None => Vec::new(),
    }
}

/// `dc:identifier` 配列からDOIを抽出してDOI URLに整形．
pub(crate) fn extract_doi(field: Option<&Value>) -> Option<String> {
    let arr = match field? {
        Value::Array(a) => a,
        _ => return None,
    };
    for ident in arr {
        let Value::Object(o) = ident else { continue };
        let ty = o.get("@type").and_then(|v| v.as_str()).unwrap_or("");
        if ty.contains("DOI")
            && let Some(val) = o.get("@value").and_then(|v| v.as_str())
            && !val.trim().is_empty()
        {
            let trimmed = val.trim();
            if trimmed.starts_with("http") {
                return Some(trimmed.to_string());
            }
            return Some(format!("https://doi.org/{}", trimmed));
        }
    }
    None
}

/// W3CDTF形式（`YYYY` または `YYYY-MM-DD` など）の先頭4桁から年を抽出．
///
/// 厳格バリデーション：
/// - 入力長が4文字でも，5文字目が `-` でもない（例 "12345"）→ 拒否
/// - 年=0（"0000"）→ 拒否
/// - 年>9999 は u16 では表現できるが，先頭4桁strict + 5文字目が `-` 必須により
///   "99999-..." 形式の入力をsilent truncationしない
pub(crate) fn extract_year(field: Option<&Value>) -> Option<u16> {
    let s = match field? {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("@value")?.as_str()?.to_string(),
        Value::Array(arr) => arr.iter().find_map(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => o.get("@value").and_then(|v| v.as_str()).map(String::from),
            _ => None,
        })?,
        _ => return None,
    };
    // W3CDTF: YYYY 単体 or YYYY- で始まる必要がある
    let bytes = s.as_bytes();
    let is_strict_w3cdtf = bytes.len() == 4 || (bytes.len() > 4 && bytes[4] == b'-');
    if !is_strict_w3cdtf {
        return None;
    }
    let year: u16 = s.get(0..4)?.parse().ok()?;
    if year == 0 {
        return None;
    }
    Some(year)
}

/// `abstractLicenseFlag` が `allow` の場合のみ `true`．
pub(crate) fn abstract_allowed(item: &Value) -> bool {
    item.get("abstractLicenseFlag")
        .and_then(|v| v.as_str())
        .map(|s| s == LICENSE_ALLOW)
        .unwrap_or(false)
}

/// `dc:subject` からカテゴリ名を重複なく抽出．
pub(crate) fn extract_subjects(field: Option<&Value>, prefer_lang: &str) -> Vec<String> {
    let mut seen = Vec::new();
    let push = |v: &Value, seen: &mut Vec<String>| {
        if let Some(s) = extract_localized_text(Some(v), prefer_lang)
            && !seen.contains(&s)
        {
            seen.push(s);
        }
    };
    match field {
        Some(Value::Array(arr)) => {
            for v in arr {
                push(v, &mut seen);
            }
        }
        Some(other) => push(other, &mut seen),
        None => {}
    }
    seen
}

// ---------------------------------------------------------------------------
// item → Paper 変換
// ---------------------------------------------------------------------------

fn compute_hash(title: &str, abstract_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(abstract_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// タイトルから不可視文字（BOM `U+FEFF`，ZWSP `U+200B`，ZWNJ `U+200C`，ZWJ `U+200D`）を除去．
///
/// CiNii から返るタイトルにこれらが混入すると，`compute_id` / `compute_hash` で
/// 同一論文が別レコードとして扱われる．正規化で重複を防ぐ．
fn normalize_title(s: &str) -> String {
    s.chars()
        .filter(|&c| !matches!(c, '\u{FEFF}' | '\u{200B}' | '\u{200C}' | '\u{200D}'))
        .collect::<String>()
        .trim()
        .to_string()
}

/// CiNii の1件分（JSON-LD item）を `Paper` に変換．
///
/// 仕様（合意済み）：
/// - タイトル/著者/カテゴリは日本語優先，無ければ英語
/// - 抄録は `abstractLicenseFlag == "allow"` の場合のみ格納（それ以外は空文字）
/// - 年が取れない場合や，タイトルが空の場合は `None` を返してスキップ
pub(crate) fn item_to_paper(item: &Value, conf_id: &str) -> Option<Paper> {
    const PREFER_LANG: &str = "ja";

    let raw_title = extract_localized_text(item.get("dc:title"), PREFER_LANG)
        .or_else(|| extract_localized_text(item.get("title"), PREFER_LANG))?;
    let title = normalize_title(&raw_title);
    if title.is_empty() {
        return None;
    }

    let year = extract_year(item.get("prism:publicationDate"))?;

    let authors = extract_creators(item.get("dc:creator"), PREFER_LANG);

    let r#abstract = if abstract_allowed(item) {
        extract_localized_text(item.get("description"), PREFER_LANG).unwrap_or_default()
    } else {
        String::new()
    };

    let url = extract_doi(item.get("dc:identifier"))
        .or_else(|| {
            item.get("@id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    let categories = extract_subjects(item.get("dc:subject"), PREFER_LANG);

    let hash = compute_hash(&title, &r#abstract);

    Some(Paper {
        id: compute_id(&title),
        conference: conf_id.to_string(),
        year,
        title,
        authors,
        r#abstract,
        url,
        // CiNii のレスポンスにはPDF直リンクが含まれないため，
        // 後段のenrichmentパイプライン（HTML→PDF）に委ねる．
        pdf_url: None,
        categories,
        hash,
    })
}

// ---------------------------------------------------------------------------
// 公開API
// ---------------------------------------------------------------------------

/// 指定 ISSN・指定年の論文を CiNii Research から全件取得．
///
/// `start` 上限（10000）に達した場合は警告ログを出して打ち切る．
/// 1ジャーナル×1年で10000件を超えるケースは現実にはほぼ発生しない．
pub(crate) async fn fetch_papers_for_year(
    client: &reqwest::Client,
    issn: &str,
    appid: &str,
    conf_id: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    fetch_papers_for_year_with_endpoint(client, ENDPOINT, issn, appid, conf_id, year, interval)
        .await
}

/// `fetch_papers_for_year` の内側．エンドポイントURLを差し替え可能にしてテストでモックする．
async fn fetch_papers_for_year_with_endpoint(
    client: &reqwest::Client,
    endpoint: &str,
    issn: &str,
    appid: &str,
    conf_id: &str,
    year: u16,
    interval: Duration,
) -> Result<Vec<Paper>> {
    let mut papers = Vec::new();
    let mut start: u32 = 1;

    loop {
        if start > MAX_START {
            tracing::warn!(
                "CiNii Research: hit start={} cap for issn={} year={}, results may be truncated",
                MAX_START,
                issn,
                year
            );
            break;
        }

        let mut url =
            reqwest::Url::parse(endpoint).context("Failed to parse CiNii Research endpoint URL")?;
        url.query_pairs_mut()
            .append_pair("appid", appid)
            .append_pair("issn", issn)
            .append_pair("from", &year.to_string())
            .append_pair("until", &year.to_string())
            .append_pair("count", &PAGE_SIZE.to_string())
            .append_pair("start", &start.to_string())
            .append_pair("format", "json")
            .append_pair("lang", "ja");

        let body = fetch_with_sleep(client, url.as_str(), interval)
            .await
            .with_context(|| {
                format!(
                    "CiNii Research fetch failed for issn={} year={} start={}",
                    issn, year, start
                )
            })?;

        let resp: CiniiResponse = serde_json::from_str(&body)
            .with_context(|| format!("Failed to parse CiNii Research response (start={})", start))?;

        let items = resp.results();
        if items.is_empty() {
            break;
        }

        for item in items {
            if let Some(p) = item_to_paper(item, conf_id) {
                papers.push(p);
            }
        }

        // ページ未満しか返らなければ終端
        if (items.len() as u32) < PAGE_SIZE {
            break;
        }
        start = start.saturating_add(PAGE_SIZE);
    }

    Ok(papers)
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_localized_text_prefers_target_lang() {
        let v = json!([
            {"@value": "English Title", "@language": "en"},
            {"@value": "日本語タイトル", "@language": "ja"}
        ]);
        assert_eq!(
            extract_localized_text(Some(&v), "ja"),
            Some("日本語タイトル".to_string())
        );
    }

    #[test]
    fn extract_localized_text_falls_back_when_lang_missing() {
        let v = json!([
            {"@value": "Only English", "@language": "en"}
        ]);
        assert_eq!(
            extract_localized_text(Some(&v), "ja"),
            Some("Only English".to_string())
        );
    }

    #[test]
    fn extract_localized_text_handles_plain_string() {
        let v = json!("Plain Title");
        assert_eq!(
            extract_localized_text(Some(&v), "ja"),
            Some("Plain Title".to_string())
        );
    }

    #[test]
    fn extract_localized_text_skips_empty() {
        let v = json!([
            {"@value": "  ", "@language": "ja"},
            {"@value": "Real", "@language": "en"}
        ]);
        assert_eq!(
            extract_localized_text(Some(&v), "ja"),
            Some("Real".to_string())
        );
    }

    #[test]
    fn extract_localized_text_none_for_missing() {
        assert_eq!(extract_localized_text(None, "ja"), None);
    }

    #[test]
    fn extract_creators_handles_nested_lang_arrays() {
        // 1著者が言語別の入れ子配列で表現されるケース
        let v = json!([
            [
                {"@value": "山田 太郎", "@language": "ja"},
                {"@value": "Yamada Taro", "@language": "en"}
            ],
            [
                {"@value": "鈴木 花子", "@language": "ja"},
                {"@value": "Suzuki Hanako", "@language": "en"}
            ]
        ]);
        assert_eq!(
            extract_creators(Some(&v), "ja"),
            vec!["山田 太郎".to_string(), "鈴木 花子".to_string()]
        );
    }

    #[test]
    fn extract_creators_returns_empty_vec_for_none() {
        // 公開契約：None入力 → 空Vec（panicしない）
        let got: Vec<String> = extract_creators(None, "ja");
        assert!(got.is_empty());
    }

    #[test]
    fn extract_subjects_returns_empty_vec_for_none() {
        let got: Vec<String> = extract_subjects(None, "ja");
        assert!(got.is_empty());
    }

    #[test]
    fn extract_doi_returns_none_for_none_input() {
        assert_eq!(extract_doi(None), None);
    }

    #[test]
    fn extract_creators_handles_flat_string_array() {
        let v = json!(["Alice", "Bob"]);
        assert_eq!(
            extract_creators(Some(&v), "ja"),
            vec!["Alice".to_string(), "Bob".to_string()]
        );
    }

    #[test]
    fn extract_doi_finds_doi_typed_identifier() {
        let v = json!([
            {"@value": "1234-5678", "@type": "https://cir.nii.ac.jp/schema/1.0/ISSN"},
            {"@value": "10.1234/jsai.39.001", "@type": "https://cir.nii.ac.jp/schema/1.0/DOI"}
        ]);
        assert_eq!(
            extract_doi(Some(&v)),
            Some("https://doi.org/10.1234/jsai.39.001".to_string())
        );
    }

    #[test]
    fn extract_doi_passes_through_full_url() {
        let v = json!([
            {"@value": "https://doi.org/10.1234/jsai.39.001", "@type": "https://cir.nii.ac.jp/schema/1.0/DOI"}
        ]);
        assert_eq!(
            extract_doi(Some(&v)),
            Some("https://doi.org/10.1234/jsai.39.001".to_string())
        );
    }

    #[test]
    fn extract_doi_returns_none_when_no_doi() {
        let v = json!([
            {"@value": "1234-5678", "@type": "https://cir.nii.ac.jp/schema/1.0/ISSN"}
        ]);
        assert_eq!(extract_doi(Some(&v)), None);
    }

    #[test]
    fn extract_year_parses_w3cdtf() {
        assert_eq!(extract_year(Some(&json!("2024-03-15"))), Some(2024));
        assert_eq!(extract_year(Some(&json!("2020"))), Some(2020));
        assert_eq!(extract_year(Some(&json!("2024-03"))), Some(2024));
    }

    #[test]
    fn extract_year_handles_object() {
        let v = json!({"@value": "2024-01-01", "@language": "ja"});
        assert_eq!(extract_year(Some(&v)), Some(2024));
    }

    #[test]
    fn extract_year_returns_none_for_garbage() {
        assert_eq!(extract_year(Some(&json!("not-a-year"))), None);
        assert_eq!(extract_year(None), None);
    }

    #[test]
    fn extract_year_rejects_year_zero() {
        // "0000-01-01" は形式上valid W3CDTFだが意味的な年=0は拒否
        assert_eq!(extract_year(Some(&json!("0000-01-01"))), None);
        assert_eq!(extract_year(Some(&json!("0000"))), None);
    }

    #[test]
    fn extract_year_rejects_5plus_digit_year() {
        // 5桁以上の年は silent truncation を許さない
        assert_eq!(extract_year(Some(&json!("99999-01-01"))), None);
        assert_eq!(extract_year(Some(&json!("12345"))), None);
    }

    #[test]
    fn extract_year_accepts_valid_boundary_years() {
        // 1 と 9999 は valid
        assert_eq!(extract_year(Some(&json!("0001-01-01"))), Some(1));
        assert_eq!(extract_year(Some(&json!("9999-12-31"))), Some(9999));
        assert_eq!(extract_year(Some(&json!("9999"))), Some(9999));
    }

    #[test]
    fn item_to_paper_skips_when_year_is_zero() {
        let item = json!({
            "dc:title": [{"@value": "年=0論文", "@language": "ja"}],
            "prism:publicationDate": "0000-01-01"
        });
        assert!(item_to_paper(&item, "tjsai").is_none());
    }

    #[test]
    fn abstract_allowed_only_for_allow_value() {
        assert!(abstract_allowed(&json!({"abstractLicenseFlag": "allow"})));
        assert!(!abstract_allowed(&json!({"abstractLicenseFlag": "disallow"})));
        assert!(!abstract_allowed(&json!({"abstractLicenseFlag": ""})));
        assert!(!abstract_allowed(&json!({})));
    }

    #[test]
    fn extract_subjects_dedupes_identical_entries() {
        // CiNii の dc:subject は通常フラット配列で各言語が別エントリ．
        // 同一文字列は重複排除し，言語違いは別カテゴリとして残す．
        let v = json!([
            {"@value": "深層学習", "@language": "ja"},
            {"@value": "Deep Learning", "@language": "en"},
            {"@value": "深層学習", "@language": "ja"}
        ]);
        assert_eq!(
            extract_subjects(Some(&v), "ja"),
            vec!["深層学習".to_string(), "Deep Learning".to_string()]
        );
    }

    #[test]
    fn extract_subjects_picks_japanese_from_nested_variants() {
        // 1サブジェクトが言語別バリアントを内包する場合は ja を採用
        let v = json!([
            [
                {"@value": "深層学習", "@language": "ja"},
                {"@value": "Deep Learning", "@language": "en"}
            ]
        ]);
        assert_eq!(extract_subjects(Some(&v), "ja"), vec!["深層学習".to_string()]);
    }

    // [G] F-10: BOM/ゼロ幅スペースで重複レコード化を防ぐ正規化
    #[test]
    fn item_to_paper_normalizes_bom_in_title() {
        let with_bom = json!({
            "dc:title": [{"@value": "\u{FEFF}機械学習入門", "@language": "ja"}],
            "prism:publicationDate": "2024",
            "abstractLicenseFlag": "disallow"
        });
        let without_bom = json!({
            "dc:title": [{"@value": "機械学習入門", "@language": "ja"}],
            "prism:publicationDate": "2024",
            "abstractLicenseFlag": "disallow"
        });
        let p1 = item_to_paper(&with_bom, "tjsai").unwrap();
        let p2 = item_to_paper(&without_bom, "tjsai").unwrap();
        assert_eq!(p1.title, p2.title, "BOM should be stripped from title");
        assert_eq!(p1.id, p2.id, "id (sha256(title)) should be identical");
        assert_eq!(p1.hash, p2.hash, "hash should be identical");
    }

    #[test]
    fn item_to_paper_normalizes_zero_width_space_in_title() {
        let with_zwsp = json!({
            "dc:title": [{"@value": "深層\u{200B}学習", "@language": "ja"}],
            "prism:publicationDate": "2024",
            "abstractLicenseFlag": "disallow"
        });
        let without = json!({
            "dc:title": [{"@value": "深層学習", "@language": "ja"}],
            "prism:publicationDate": "2024",
            "abstractLicenseFlag": "disallow"
        });
        let p1 = item_to_paper(&with_zwsp, "tjsai").unwrap();
        let p2 = item_to_paper(&without, "tjsai").unwrap();
        assert_eq!(p1.id, p2.id);
    }

    #[test]
    fn item_to_paper_full_record() {
        let item = json!({
            "@id": "https://cir.nii.ac.jp/crid/1234567890",
            "@type": "article",
            "dc:title": [
                {"@value": "深層学習を用いた感情分析", "@language": "ja"},
                {"@value": "Sentiment Analysis with Deep Learning", "@language": "en"}
            ],
            "dc:creator": [
                [
                    {"@value": "山田 太郎", "@language": "ja"},
                    {"@value": "Yamada Taro", "@language": "en"}
                ],
                [
                    {"@value": "鈴木 花子", "@language": "ja"}
                ]
            ],
            "description": [
                {"@value": "本論文では...", "@language": "ja"},
                {"@value": "In this paper...", "@language": "en"}
            ],
            "abstractLicenseFlag": "allow",
            "prism:publicationDate": "2024-03-15",
            "dc:identifier": [
                {"@value": "10.1527/tjsai.39.001", "@type": "https://cir.nii.ac.jp/schema/1.0/DOI"}
            ],
            "dc:subject": [
                {"@value": "深層学習", "@language": "ja"}
            ]
        });

        let p = item_to_paper(&item, "tjsai").unwrap();
        assert_eq!(p.title, "深層学習を用いた感情分析");
        assert_eq!(p.authors, vec!["山田 太郎", "鈴木 花子"]);
        assert_eq!(p.r#abstract, "本論文では...");
        assert_eq!(p.url, "https://doi.org/10.1527/tjsai.39.001");
        assert_eq!(p.year, 2024);
        assert_eq!(p.conference, "tjsai");
        assert_eq!(p.categories, vec!["深層学習"]);
        assert!(p.pdf_url.is_none());
        assert_eq!(p.id, compute_id("深層学習を用いた感情分析"));
        assert_eq!(p.hash.len(), 64);
    }

    #[test]
    fn item_to_paper_skips_abstract_when_disallow() {
        let item = json!({
            "dc:title": [{"@value": "ライセンス制限あり論文", "@language": "ja"}],
            "description": [{"@value": "本論文は...", "@language": "ja"}],
            "abstractLicenseFlag": "disallow",
            "prism:publicationDate": "2024"
        });
        let p = item_to_paper(&item, "tjsai").unwrap();
        assert_eq!(p.r#abstract, "");
        assert_eq!(p.title, "ライセンス制限あり論文");
    }

    #[test]
    fn item_to_paper_skips_when_year_missing() {
        let item = json!({
            "dc:title": [{"@value": "年なし論文", "@language": "ja"}]
        });
        assert!(item_to_paper(&item, "tjsai").is_none());
    }

    #[test]
    fn item_to_paper_skips_when_title_missing() {
        let item = json!({
            "prism:publicationDate": "2024"
        });
        assert!(item_to_paper(&item, "tjsai").is_none());
    }

    #[test]
    fn item_to_paper_falls_back_to_at_id_when_no_doi() {
        let item = json!({
            "@id": "https://cir.nii.ac.jp/crid/1234567890",
            "dc:title": [{"@value": "DOIなし論文", "@language": "ja"}],
            "prism:publicationDate": "2024",
            "abstractLicenseFlag": "disallow"
        });
        let p = item_to_paper(&item, "tjsai").unwrap();
        assert_eq!(p.url, "https://cir.nii.ac.jp/crid/1234567890");
    }

    #[test]
    fn parse_response_with_at_graph() {
        let body = r#"{
            "@graph": [
                {"@type": "article", "dc:title": [{"@value": "A", "@language": "ja"}], "prism:publicationDate": "2024"}
            ]
        }"#;
        let r: CiniiResponse = serde_json::from_str(body).unwrap();
        assert_eq!(r.results().len(), 1);
    }

    #[test]
    fn parse_response_with_items_field() {
        let body = r#"{
            "items": [
                {"dc:title": "B", "prism:publicationDate": "2024"}
            ]
        }"#;
        let r: CiniiResponse = serde_json::from_str(body).unwrap();
        assert_eq!(r.results().len(), 1);
    }

    #[test]
    fn parse_response_empty() {
        let body = r#"{"@graph": []}"#;
        let r: CiniiResponse = serde_json::from_str(body).unwrap();
        assert!(r.results().is_empty());
    }

    // -----------------------------------------------------------------------
    // wiremock-rs を用いた HTTP 統合テスト
    // -----------------------------------------------------------------------

    /// テスト用に短い間隔で fetch_papers_for_year_with_endpoint を呼ぶ
    async fn run_fetch(endpoint: &str) -> Result<Vec<Paper>> {
        let client = reqwest::Client::new();
        fetch_papers_for_year_with_endpoint(
            &client,
            endpoint,
            "1346-8030",
            "test-appid",
            "tjsai",
            2024,
            Duration::from_millis(0),
        )
        .await
    }

    #[tokio::test]
    async fn fetch_propagates_cinii_specific_error_context_on_5xx() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/articles"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&mock_server)
            .await;

        let endpoint = format!("{}/articles", mock_server.uri());
        let err = run_fetch(&endpoint).await.unwrap_err();
        let chain: String = format!("{:?}", err);
        assert!(
            chain.contains("CiNii Research fetch failed"),
            "expected cinii-specific context, got: {}",
            chain
        );
        assert!(chain.contains("issn=1346-8030"), "issn should be in context: {}", chain);
        assert!(chain.contains("year=2024"), "year should be in context: {}", chain);
        assert!(chain.contains("start=1"), "start should be in context: {}", chain);
    }

    /// PAGE_SIZE 件分の最小valid応答を生成
    fn make_full_page_response(prefix: &str) -> serde_json::Value {
        let items: Vec<serde_json::Value> = (0..PAGE_SIZE)
            .map(|i| {
                json!({
                    "dc:title": [{"@value": format!("{}-{}", prefix, i), "@language": "ja"}],
                    "prism:publicationDate": "2024"
                })
            })
            .collect();
        json!({ "@graph": items })
    }

    #[tokio::test]
    async fn fetch_stops_at_max_start_boundary() {
        // MAX_START=10000 / PAGE_SIZE=200 → 50ページが上限．
        // 51ページ目（start=10001）のリクエストは発火しないはず．
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let mock_server = MockServer::start().await;
        let body = make_full_page_response("p");
        Mock::given(method("GET"))
            .and(path("/articles"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(50_u64) // 必ず50回のみ
            .mount(&mock_server)
            .await;

        let endpoint = format!("{}/articles", mock_server.uri());
        let papers = run_fetch(&endpoint).await.unwrap();
        // 50ページ × 200件 = 10000件
        assert_eq!(
            papers.len(),
            (MAX_START as usize / PAGE_SIZE as usize) * PAGE_SIZE as usize,
            "expected 10000 papers from 50 pages"
        );
        // mock_server drop時に expect(50) が検証される
    }

    // [D] F-08: @graph と items が両方非空のときの優先順位
    #[test]
    fn parse_response_prefers_graph_over_items_when_both_present() {
        let body = r#"{
            "@graph": [
                {"dc:title": [{"@value": "FromGraph", "@language": "ja"}], "prism:publicationDate": "2024"}
            ],
            "items": [
                {"dc:title": [{"@value": "FromItems", "@language": "ja"}], "prism:publicationDate": "2024"}
            ]
        }"#;
        let r: CiniiResponse = serde_json::from_str(body).unwrap();
        let results = r.results();
        assert_eq!(results.len(), 1, "expected only @graph items, not merge");
        assert_eq!(
            extract_localized_text(results[0].get("dc:title"), "ja").as_deref(),
            Some("FromGraph")
        );
    }

    // [E] F-11: 異なる年のリクエストが正しく分離される
    #[tokio::test]
    async fn fetch_isolates_years_correctly() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/articles"))
            .and(query_param("from", "2024"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "@graph": [{
                    "dc:title": [{"@value": "P2024", "@language": "ja"}],
                    "prism:publicationDate": "2024"
                }]
            })))
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/articles"))
            .and(query_param("from", "2025"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "@graph": [{
                    "dc:title": [{"@value": "P2025", "@language": "ja"}],
                    "prism:publicationDate": "2025"
                }]
            })))
            .mount(&mock_server)
            .await;

        let endpoint = format!("{}/articles", mock_server.uri());
        let client = reqwest::Client::new();

        let papers_2024 = fetch_papers_for_year_with_endpoint(
            &client,
            &endpoint,
            "1346-8030",
            "appid",
            "tjsai",
            2024,
            Duration::from_millis(0),
        )
        .await
        .unwrap();
        assert_eq!(papers_2024.len(), 1);
        assert_eq!(papers_2024[0].title, "P2024");
        assert_eq!(papers_2024[0].year, 2024);

        let papers_2025 = fetch_papers_for_year_with_endpoint(
            &client,
            &endpoint,
            "1346-8030",
            "appid",
            "tjsai",
            2025,
            Duration::from_millis(0),
        )
        .await
        .unwrap();
        assert_eq!(papers_2025.len(), 1);
        assert_eq!(papers_2025[0].title, "P2025");
        assert_eq!(papers_2025[0].year, 2025);
    }

    // [F] F-03 / H-20260503-01: 無効appid沈黙失敗の現状behavior pinning．
    // 仕様判断保留中．現実装は `Ok(vec![])` を返す．
    #[tokio::test]
    async fn fetch_returns_ok_empty_for_silent_failure_response() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let mock_server = MockServer::start().await;
        // 200 OK + 空オブジェクト（@graph も items も無い）
        Mock::given(method("GET"))
            .and(path("/articles"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&mock_server)
            .await;

        let endpoint = format!("{}/articles", mock_server.uri());
        let papers = run_fetch(&endpoint).await.unwrap();
        // 既知の限界：認証失敗等を区別できない．将来の仕様変更でこのテストを更新する．
        assert_eq!(papers.len(), 0, "current behavior pinning, see F-20260503-03");
    }

    #[tokio::test]
    async fn fetch_propagates_parse_error_context_on_invalid_json() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/articles"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
            .mount(&mock_server)
            .await;

        let endpoint = format!("{}/articles", mock_server.uri());
        let err = run_fetch(&endpoint).await.unwrap_err();
        let chain = format!("{:?}", err);
        assert!(
            chain.contains("Failed to parse CiNii Research response"),
            "expected parse-error context, got: {}",
            chain
        );
        assert!(chain.contains("start=1"), "start should be in context: {}", chain);
    }
}
