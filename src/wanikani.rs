use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode, header};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub const DEFAULT_WK_API_BASE: &str = "https://api.wanikani.com/v2";
const MAX_RETRY_ATTEMPTS: u32 = 5;
const MAX_BACKOFF_SECS: u64 = 30;
const SUBJECT_IDS_BATCH_SIZE: usize = 500;

#[derive(Debug, Deserialize)]
struct WkUserEnvelope {
    data: WkUserData,
}

#[derive(Debug, Deserialize)]
struct WkUserData {
    username: String,
    level: i32,
}

#[derive(Debug, Deserialize)]
struct WkSummaryEnvelope {
    data: WkSummaryData,
}

#[derive(Debug, Deserialize)]
struct WkSummaryData {
    lessons: Vec<WkSummaryBucket>,
    reviews: Vec<WkSummaryBucket>,
}

#[derive(Debug, Deserialize)]
struct WkSummaryBucket {
    subject_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct WkCollectionEnvelope<T> {
    data_updated_at: Option<String>,
    pages: WkPages,
    data: Vec<WkResource<T>>,
}

#[derive(Debug, Deserialize)]
struct WkPages {
    next_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WkResource<T> {
    id: i64,
    data: T,
}

#[derive(Debug, Deserialize)]
struct WkAssignmentData {
    subject_id: i64,
    subject_type: String,
    srs_stage: i32,
    passed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WkSubjectData {
    characters: Option<String>,
    level: i32,
}

/// Stored incremental sync state from `wk_profile` used for conditional requests & `updated_after`.
#[derive(Debug, Clone, Default)]
pub struct WaniKaniSyncCursor {
    pub user_etag: Option<String>,
    pub summary_etag: Option<String>,
    pub assignments_updated_after: Option<DateTime<Utc>>,
    pub assignments_etag: Option<String>,
    pub subjects_updated_after: Option<DateTime<Utc>>,
    pub subjects_etag: Option<String>,
    pub cached_username: Option<String>,
    pub cached_level: i32,
    pub cached_lessons: i32,
    pub cached_reviews: i32,
}

/// Live progress event emitted during background WaniKani syncs.
#[derive(Debug, Clone)]
pub struct WaniKaniSyncProgress {
    pub pages_fetched: i32,
    pub items_fetched: i32,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct WaniKaniSyncResult {
    pub username: String,
    pub level: i32,
    pub lessons_available: i32,
    pub reviews_available: i32,
    pub subjects: Vec<(i64, String, String, i32, i32)>, // (subject_id, type, characters, level, srs_stage)
    pub removed_subject_ids: Vec<i64>,
    pub is_incremental: bool,
    pub user_etag: Option<String>,
    pub summary_etag: Option<String>,
    pub assignments_updated_after: Option<DateTime<Utc>>,
    pub assignments_etag: Option<String>,
    pub subjects_updated_after: Option<DateTime<Utc>>,
    pub subjects_etag: Option<String>,
}

enum FetchOutcome {
    NotModified,
    Ok {
        body: String,
        etag: Option<String>,
    },
}

/// Hardened WaniKani API v2 client supporting unbounded pagination, conditional ETag /
/// If-Modified-Since requests, incremental `updated_after`, and HTTP 429 `Retry-After` backoff.
#[derive(Clone)]
pub struct WaniKaniClient {
    client: Client,
    api_base_url: String,
    retry_base_ms: u64,
}

impl Default for WaniKaniClient {
    fn default() -> Self {
        Self::new(DEFAULT_WK_API_BASE.to_string(), 500)
            .expect("default WaniKani HTTP client should build")
    }
}

impl WaniKaniClient {
    pub fn new(api_base_url: String, retry_base_ms: u64) -> Result<Self> {
        let client = Client::builder()
            .user_agent("BunAmateur/0.2 (Japanese Grammar SRS)")
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to initialize WaniKani HTTP client")?;
        Ok(Self {
            client,
            api_base_url: api_base_url.trim_end_matches('/').to_string(),
            retry_base_ms: retry_base_ms.max(1),
        })
    }

    fn parse_retry_after(headers: &header::HeaderMap) -> Option<Duration> {
        let raw = headers.get(header::RETRY_AFTER)?.to_str().ok()?.trim();
        if let Ok(secs) = raw.parse::<u64>() {
            return Some(Duration::from_secs(secs.min(MAX_BACKOFF_SECS)));
        }
        if let Ok(target) = DateTime::parse_from_rfc2822(raw) {
            let delta = (target.with_timezone(&Utc) - Utc::now()).num_seconds().max(0) as u64;
            return Some(Duration::from_secs(delta.min(MAX_BACKOFF_SECS)));
        }
        None
    }

    async fn get_with_retry(
        &self,
        url: &str,
        auth: &str,
        etag: Option<&str>,
        if_modified_since: Option<DateTime<Utc>>,
    ) -> Result<FetchOutcome> {
        let mut attempt: u32 = 0;

        loop {
            attempt += 1;
            let mut req = self
                .client
                .get(url)
                .header(header::AUTHORIZATION, auth)
                .header("Wanikani-Revision", "20170710");

            if let Some(tag) = etag
                && !tag.is_empty()
            {
                req = req.header(header::IF_NONE_MATCH, tag);
            }
            if let Some(ims) = if_modified_since {
                let http_date = ims.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
                req = req.header(header::IF_MODIFIED_SINCE, http_date);
            }

            let resp = req
                .send()
                .await
                .with_context(|| format!("Failed to reach WaniKani API ({})", url))?;

            let status = resp.status();
            if status == StatusCode::NOT_MODIFIED {
                return Ok(FetchOutcome::NotModified);
            }

            if status == StatusCode::TOO_MANY_REQUESTS
                || matches!(
                    status,
                    StatusCode::BAD_GATEWAY
                        | StatusCode::SERVICE_UNAVAILABLE
                        | StatusCode::GATEWAY_TIMEOUT
                )
            {
                if attempt >= MAX_RETRY_ATTEMPTS {
                    return Err(anyhow!(
                        "WaniKani API returned HTTP {} after {} attempts",
                        status.as_u16(),
                        attempt
                    ));
                }

                let wait = Self::parse_retry_after(resp.headers()).unwrap_or_else(|| {
                    let mult = 1u64.checked_shl(attempt - 1).unwrap_or(16);
                    let ms = self
                        .retry_base_ms
                        .saturating_mul(mult)
                        .min(MAX_BACKOFF_SECS * 1000);
                    Duration::from_millis(ms)
                });
                tracing::warn!(
                    status = status.as_u16(),
                    attempt,
                    wait_ms = wait.as_millis() as u64,
                    "WaniKani API rate-limited or transiently unavailable; backing off"
                );
                tokio::time::sleep(wait).await;
                continue;
            }

            if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                return Err(anyhow!(
                    "WaniKani authentication failed (HTTP {}). Check your Personal Access Token.",
                    status.as_u16()
                ));
            }

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                let snippet: String = body.chars().take(240).collect();
                return Err(anyhow!(
                    "WaniKani API returned HTTP {}: {}",
                    status.as_u16(),
                    snippet
                ));
            }

            let new_etag = resp
                .headers()
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let body = resp.text().await?;
            return Ok(FetchOutcome::Ok {
                body,
                etag: new_etag,
            });
        }
    }

    /// Lightweight token verification against `/user` before queuing a background sync.
    pub async fn verify_token(&self, api_token: &str) -> Result<(String, i32, Option<String>)> {
        let auth = format!("Bearer {}", api_token.trim());
        let url = format!("{}/user", self.api_base_url);
        match self.get_with_retry(&url, &auth, None, None).await? {
            FetchOutcome::Ok { body, etag } => {
                let user: WkUserEnvelope =
                    serde_json::from_str(&body).context("Failed to parse WaniKani /user payload")?;
                Ok((user.data.username, user.data.level, etag))
            }
            FetchOutcome::NotModified => {
                Err(anyhow!("Unexpected 304 Not Modified during token verification"))
            }
        }
    }

    /// Executes a full or incremental WaniKani API v2 sync with unbounded pagination,
    /// ETag / If-Modified-Since conditional checks, `updated_after` delta fetches,
    /// HTTP 429 exponential backoff, and live progress reporting.
    pub async fn sync_with_progress<F>(
        &self,
        api_token: &str,
        cursor: &WaniKaniSyncCursor,
        force_full: bool,
        mut on_progress: F,
    ) -> Result<WaniKaniSyncResult>
    where
        F: FnMut(WaniKaniSyncProgress),
    {
        let auth = format!("Bearer {}", api_token.trim());
        let is_incremental = !force_full && cursor.assignments_updated_after.is_some();
        let mut pages_fetched: i32 = 0;
        let mut items_fetched: i32 = 0;

        on_progress(WaniKaniSyncProgress {
            pages_fetched,
            items_fetched,
            message: if is_incremental {
                "Checking WaniKani profile & incremental updates...".to_string()
            } else {
                "Fetching WaniKani profile...".to_string()
            },
        });

        // 1. Fetch User Profile (with ETag conditional request)
        let user_url = format!("{}/user", self.api_base_url);
        let (username, level, user_etag) = match self
            .get_with_retry(
                &user_url,
                &auth,
                if force_full { None } else { cursor.user_etag.as_deref() },
                None,
            )
            .await?
        {
            FetchOutcome::NotModified => (
                cursor
                    .cached_username
                    .clone()
                    .unwrap_or_else(|| "WaniKani Learner".to_string()),
                cursor.cached_level.max(1),
                cursor.user_etag.clone(),
            ),
            FetchOutcome::Ok { body, etag } => {
                let user: WkUserEnvelope = serde_json::from_str(&body)?;
                (user.data.username, user.data.level, etag)
            }
        };
        pages_fetched += 1;

        // 2. Fetch Summary (available lessons & reviews now) with ETag
        let summary_url = format!("{}/summary", self.api_base_url);
        let (lessons_available, reviews_available, summary_etag) = match self
            .get_with_retry(
                &summary_url,
                &auth,
                if force_full { None } else { cursor.summary_etag.as_deref() },
                None,
            )
            .await
        {
            Ok(FetchOutcome::NotModified) => (
                cursor.cached_lessons,
                cursor.cached_reviews,
                cursor.summary_etag.clone(),
            ),
            Ok(FetchOutcome::Ok { body, etag }) => {
                let summary: WkSummaryEnvelope = serde_json::from_str(&body)?;
                let lessons = summary
                    .data
                    .lessons
                    .first()
                    .map(|b| b.subject_ids.len() as i32)
                    .unwrap_or(0);
                let reviews = summary
                    .data
                    .reviews
                    .first()
                    .map(|b| b.subject_ids.len() as i32)
                    .unwrap_or(0);
                (lessons, reviews, etag)
            }
            Err(e) => {
                tracing::warn!("WaniKani /summary fetch skipped due to error: {}", e);
                (cursor.cached_lessons, cursor.cached_reviews, cursor.summary_etag.clone())
            }
        };
        pages_fetched += 1;

        // 3. Fetch assignments (full or incremental via updated_after) — NO 20-page cap!
        let mut passed_map: HashMap<i64, (String, i32)> = HashMap::new();
        let mut removed_subject_ids: Vec<i64> = Vec::new();
        let mut assignments_updated_after = cursor.assignments_updated_after;
        let mut assignments_etag = cursor.assignments_etag.clone();

        let initial_assignments_url = if is_incremental {
            let since = cursor.assignments_updated_after.unwrap().to_rfc3339();
            format!(
                "{}/assignments?subject_types=kanji,vocabulary&updated_after={}",
                self.api_base_url,
                urlencoding_rfc3339(&since)
            )
        } else {
            format!(
                "{}/assignments?subject_types=kanji,vocabulary&passed=true",
                self.api_base_url
            )
        };

        let mut next_url = Some(initial_assignments_url);
        let mut visited_urls: HashSet<String> = HashSet::new();
        let mut first_page = true;

        while let Some(url) = next_url {
            if !visited_urls.insert(url.clone()) {
                tracing::warn!(url = %url, "Detected repeated pagination URL in WaniKani assignments; stopping");
                break;
            }

            let outcome = self
                .get_with_retry(
                    &url,
                    &auth,
                    if first_page && is_incremental {
                        cursor.assignments_etag.as_deref()
                    } else {
                        None
                    },
                    if first_page && is_incremental {
                        cursor.assignments_updated_after
                    } else {
                        None
                    },
                )
                .await?;

            match outcome {
                FetchOutcome::NotModified => {
                    next_url = None;
                }
                FetchOutcome::Ok { body, etag } => {
                    let page: WkCollectionEnvelope<WkAssignmentData> = serde_json::from_str(&body)?;
                    if first_page {
                        if let Some(tag) = etag {
                            assignments_etag = Some(tag);
                        }
                        if let Some(ts) = page
                            .data_updated_at
                            .as_deref()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|d| d.with_timezone(&Utc))
                        {
                            assignments_updated_after = Some(ts);
                        }
                    }

                    for item in page.data {
                        if item.data.passed_at.is_some() || item.data.srs_stage >= 5 {
                            passed_map.insert(
                                item.data.subject_id,
                                (item.data.subject_type, item.data.srs_stage),
                            );
                        } else if is_incremental {
                            removed_subject_ids.push(item.data.subject_id);
                        }
                    }

                    pages_fetched += 1;
                    items_fetched = passed_map.len() as i32;
                    on_progress(WaniKaniSyncProgress {
                        pages_fetched,
                        items_fetched,
                        message: format!(
                            "Fetched {} assignment pages ({} passed items)...",
                            pages_fetched.saturating_sub(2),
                            items_fetched
                        ),
                    });

                    next_url = page.pages.next_url;
                }
            }
            first_page = false;
        }

        if assignments_updated_after.is_none() {
            assignments_updated_after = Some(Utc::now());
        }

        // 4. Fetch subjects (full: all pages up to user's level; incremental: only newly updated subject IDs)
        let mut subjects_out = Vec::new();
        let mut subjects_updated_after = cursor.subjects_updated_after;
        let mut subjects_etag = cursor.subjects_etag.clone();

        if is_incremental {
            let mut ids: Vec<i64> = passed_map.keys().copied().collect();
            ids.sort_unstable();
            for chunk in ids.chunks(SUBJECT_IDS_BATCH_SIZE) {
                let ids_param = chunk
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let mut subj_url = Some(format!(
                    "{}/subjects?ids={}",
                    self.api_base_url, ids_param
                ));
                let mut visited_subj = HashSet::new();
                while let Some(url) = subj_url {
                    if !visited_subj.insert(url.clone()) {
                        break;
                    }
                    match self.get_with_retry(&url, &auth, None, None).await? {
                        FetchOutcome::NotModified => break,
                        FetchOutcome::Ok { body, etag } => {
                            let page: WkCollectionEnvelope<WkSubjectData> =
                                serde_json::from_str(&body)?;
                            if let Some(tag) = etag {
                                subjects_etag = Some(tag);
                            }
                            if let Some(ts) = page
                                .data_updated_at
                                .as_deref()
                                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                .map(|d| d.with_timezone(&Utc))
                            {
                                subjects_updated_after = Some(ts);
                            }
                            for item in page.data {
                                if let Some((stype, srs)) = passed_map.get(&item.id)
                                    && let Some(chars) = item.data.characters
                                {
                                    subjects_out.push((
                                        item.id,
                                        stype.clone(),
                                        chars,
                                        item.data.level,
                                        *srs,
                                    ));
                                }
                            }
                            pages_fetched += 1;
                            items_fetched = subjects_out.len() as i32;
                            on_progress(WaniKaniSyncProgress {
                                pages_fetched,
                                items_fetched,
                                message: format!(
                                    "Resolved {} updated kanji & vocabulary subjects...",
                                    subjects_out.len()
                                ),
                            });
                            subj_url = page.pages.next_url;
                        }
                    }
                }
            }
        } else {
            let levels_param: String = (1..=level.clamp(1, 60))
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join(",");

            let mut subj_url = Some(format!(
                "{}/subjects?types=kanji,vocabulary&levels={}",
                self.api_base_url, levels_param
            ));
            let mut visited_subj = HashSet::new();
            let mut first_subj_page = true;

            while let Some(url) = subj_url {
                if !visited_subj.insert(url.clone()) {
                    tracing::warn!(url = %url, "Detected repeated pagination URL in WaniKani subjects; stopping");
                    break;
                }
                match self.get_with_retry(&url, &auth, None, None).await? {
                    FetchOutcome::NotModified => break,
                    FetchOutcome::Ok { body, etag } => {
                        let page: WkCollectionEnvelope<WkSubjectData> =
                            serde_json::from_str(&body)?;
                        if first_subj_page {
                            if let Some(tag) = etag {
                                subjects_etag = Some(tag);
                            }
                            if let Some(ts) = page
                                .data_updated_at
                                .as_deref()
                                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                .map(|d| d.with_timezone(&Utc))
                            {
                                subjects_updated_after = Some(ts);
                            }
                        }
                        for item in page.data {
                            if let Some((stype, srs)) = passed_map.get(&item.id)
                                && let Some(chars) = item.data.characters
                            {
                                subjects_out.push((
                                    item.id,
                                    stype.clone(),
                                    chars,
                                    item.data.level,
                                    *srs,
                                ));
                            }
                        }
                        pages_fetched += 1;
                        items_fetched = subjects_out.len() as i32;
                        on_progress(WaniKaniSyncProgress {
                            pages_fetched,
                            items_fetched,
                            message: format!(
                                "Synced {} kanji & vocabulary subjects ({} pages)...",
                                subjects_out.len(),
                                pages_fetched
                            ),
                        });
                        subj_url = page.pages.next_url;
                    }
                }
                first_subj_page = false;
            }
        }

        if subjects_updated_after.is_none() {
            subjects_updated_after = Some(Utc::now());
        }

        Ok(WaniKaniSyncResult {
            username,
            level,
            lessons_available,
            reviews_available,
            subjects: subjects_out,
            removed_subject_ids,
            is_incremental,
            user_etag,
            summary_etag,
            assignments_updated_after,
            assignments_etag,
            subjects_updated_after,
            subjects_etag,
        })
    }
}

fn urlencoding_rfc3339(ts: &str) -> String {
    ts.replace('+', "%2B").replace(':', "%3A")
}

/// Convenience helper for direct full syncs using default WaniKani v2 endpoint.
pub async fn sync_from_wanikani(api_token: &str) -> Result<WaniKaniSyncResult> {
    let client = WaniKaniClient::default();
    client
        .sync_with_progress(api_token, &WaniKaniSyncCursor::default(), true, |_| {})
        .await
}

/// Demo WaniKani Level 14 sync so users can experience Smart Furigana immediately without an API token.
pub fn generate_demo_wanikani_sync() -> WaniKaniSyncResult {
    let demo_kanji = "一二三四五六七八九十百千万上下左右中大小山川田人口目耳手足力女男子学生先生年先月火水木金土日毎時分半今何前後午間週来行帰出入見聞読書話言買売飲食立休座歩走起寝働作使思知考教習勉強漢字符文言葉名国京都会社店駅電車道車家室部屋学校友父母兄姉妹家族私自彼彼女方人達天気雨雪風雲空海花木林森犬猫鳥魚肉茶酒飯朝昼晩夜春秋冬新古高安長短多小少早遅明暗広狭近遠強弱重軽暑寒暖涼好悪正楽面白忙静賑有名親切便利元気病気薬病院医者体頭顔声心気持味色白黒赤青黄緑映画音楽写真歌旅行地図切符空港飛行機ホテル予約料理皿箸砂糖塩野菜果物牛乳卵パン米仕事会社員部長社長会議資料電話携帯メール手紙箱袋鍵傘時計財布眼鏡服靴帽子買物払値段円無料必要大切簡単難問題質問答試験合格練習準備約束計画決予定開始終了続止変直治手伝助待送届受取貸借返忘覚思い出落拾壊消点開閉乗降渡曲進戻着出発到着参加出席欠席経験趣味運動水泳野球散歩掃除洗濯運転紹介説明相談連絡報告確認注意危険安全規則禁止許可理由原因結果目的方法意味関係影響変化成長努力成功失敗応援感謝挨拶";

    let mut subjects = Vec::new();
    let mut id_counter: i64 = 1000;
    for (idx, ch) in demo_kanji.chars().enumerate() {
        let lvl = ((idx / 24) as i32 + 1).min(14);
        let srs = if lvl <= 8 { 8 } else if lvl <= 12 { 6 } else { 5 };
        subjects.push((id_counter, "kanji".to_string(), ch.to_string(), lvl, srs));
        id_counter += 1;
    }

    let demo_vocab = [
        "日本語", "毎日", "勉強", "漢字", "新聞", "電車", "会社", "友達", "天気", "雨",
        "映画", "音楽", "旅行", "料理", "仕事", "試験", "練習", "約束", "計画", "経験",
        "趣味", "運動", "散歩", "掃除", "説明", "相談", "連絡", "確認", "注意", "安全",
        "理由", "結果", "目的", "意味", "関係", "努力", "成功", "感謝", "学校", "先生",
    ];
    for (idx, v) in demo_vocab.iter().enumerate() {
        let lvl = ((idx / 3) as i32 + 1).min(14);
        subjects.push((id_counter, "vocabulary".to_string(), (*v).to_string(), lvl, 6));
        id_counter += 1;
    }

    WaniKaniSyncResult {
        username: "DemoLearner (WK Lv.14)".to_string(),
        level: 14,
        lessons_available: 12,
        reviews_available: 34,
        subjects,
        removed_subject_ids: Vec::new(),
        is_incremental: false,
        user_etag: None,
        summary_etag: None,
        assignments_updated_after: None,
        assignments_etag: None,
        subjects_updated_after: None,
        subjects_etag: None,
    }
}

pub fn is_kanji(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c) || ('\u{3400}'..='\u{4DBF}').contains(&c)
}

/// Strips `[漢字|かんじ]` ruby markup to get plain Japanese text.
pub fn strip_ruby_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            let mut j = i + 1;
            let mut pipe = None;
            let mut close = None;
            while j < chars.len() {
                if chars[j] == '|' && pipe.is_none() {
                    pipe = Some(j);
                } else if chars[j] == ']' {
                    close = Some(j);
                    break;
                }
                j += 1;
            }
            if let (Some(p), Some(c)) = (pipe, close) {
                for ch in &chars[i + 1..p] {
                    out.push(*ch);
                }
                i = c + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Returns (known_count, total_unique_kanji, readiness_percent, unknown_list)
pub fn compute_kanji_readiness(
    text: &str,
    known_kanji: &HashSet<char>,
) -> (usize, usize, u8, Vec<String>) {
    let plain = strip_ruby_markup(text);
    let mut unique = HashSet::new();
    let mut ordered_unknown = Vec::new();
    let mut seen_unknown = HashSet::new();

    for ch in plain.chars() {
        if is_kanji(ch) {
            unique.insert(ch);
            if !known_kanji.contains(&ch) && seen_unknown.insert(ch) {
                ordered_unknown.push(ch.to_string());
            }
        }
    }

    let total = unique.len();
    if total == 0 {
        return (0, 0, 100, vec![]);
    }
    let known = unique.iter().filter(|c| known_kanji.contains(c)).count();
    let pct = ((known as f64 / total as f64) * 100.0).round() as u8;
    (known, total, pct, ordered_unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::Query,
        http::{HeaderMap, HeaderValue},
        response::{IntoResponse, Response},
        routing::get,
    };
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn handles_more_than_20_pages_429_retry_and_incremental_etag_304() {
        let rate_limit_hits = Arc::new(AtomicUsize::new(0));
        let updated_after_seen = Arc::new(AtomicUsize::new(0));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        let base_for_assign = base_url.clone();
        let base_for_subj = base_url.clone();
        let rl_counter = rate_limit_hits.clone();
        let ua_counter = updated_after_seen.clone();

        let app = Router::new()
            .route(
                "/user",
                get(|headers: HeaderMap| async move {
                    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok())
                        == Some("\"user-etag-1\"")
                    {
                        return StatusCode::NOT_MODIFIED.into_response();
                    }
                    let mut resp = Json(json!({
                        "data": { "username": "kanji_master", "level": 25 }
                    }))
                    .into_response();
                    resp.headers_mut()
                        .insert(header::ETAG, HeaderValue::from_static("\"user-etag-1\""));
                    resp
                }),
            )
            .route(
                "/summary",
                get(|headers: HeaderMap| async move {
                    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok())
                        == Some("\"summary-etag-1\"")
                    {
                        return StatusCode::NOT_MODIFIED.into_response();
                    }
                    let mut resp = Json(json!({
                        "data": {
                            "lessons": [{ "subject_ids": [1, 2, 3] }],
                            "reviews": [{ "subject_ids": [4, 5] }]
                        }
                    }))
                    .into_response();
                    resp.headers_mut().insert(
                        header::ETAG,
                        HeaderValue::from_static("\"summary-etag-1\""),
                    );
                    resp
                }),
            )
            .route(
                "/assignments",
                get(move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let base = base_for_assign.clone();
                    let rl = rl_counter.clone();
                    let ua = ua_counter.clone();
                    async move {
                        // Simulate HTTP 429 twice on page 2 to verify Retry-After backoff
                        let page_num: usize = params
                            .get("page")
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(1);

                        if params.contains_key("updated_after") {
                            ua.fetch_add(1, Ordering::SeqCst);
                            if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok())
                                == Some("\"assign-etag-304\"")
                            {
                                return StatusCode::NOT_MODIFIED.into_response();
                            }
                            let mut resp = Json(json!({
                                "data_updated_at": "2026-11-25T00:10:00Z",
                                "pages": { "next_url": null },
                                "data": [
                                    {
                                        "id": 999,
                                        "data": {
                                            "subject_id": 100,
                                            "subject_type": "kanji",
                                            "srs_stage": 6,
                                            "passed_at": "2026-11-25T00:05:00Z"
                                        }
                                    },
                                    {
                                        "id": 998,
                                        "data": {
                                            "subject_id": 2,
                                            "subject_type": "kanji",
                                            "srs_stage": 1,
                                            "passed_at": null
                                        }
                                    }
                                ]
                            }))
                            .into_response();
                            resp.headers_mut().insert(
                                header::ETAG,
                                HeaderValue::from_static("\"assign-etag-304\""),
                            );
                            return resp;
                        }

                        if page_num == 2 && rl.fetch_add(1, Ordering::SeqCst) < 2 {
                            let mut resp: Response = StatusCode::TOO_MANY_REQUESTS.into_response();
                            resp.headers_mut()
                                .insert(header::RETRY_AFTER, HeaderValue::from_static("0"));
                            return resp;
                        }

                        // 23 pages total -> proves the old 20-page cap is removed
                        let total_pages = 23usize;
                        let next_url = if page_num < total_pages {
                            Some(format!("{}/assignments?page={}", base, page_num + 1))
                        } else {
                            None
                        };
                        let mut resp = Json(json!({
                            "data_updated_at": "2026-11-25T00:00:00Z",
                            "pages": { "next_url": next_url },
                            "data": [{
                                "id": page_num as i64,
                                "data": {
                                    "subject_id": page_num as i64,
                                    "subject_type": "kanji",
                                    "srs_stage": 5,
                                    "passed_at": "2026-11-01T00:00:00Z"
                                }
                            }]
                        }))
                        .into_response();
                        resp.headers_mut().insert(
                            header::ETAG,
                            HeaderValue::from_static("\"assign-etag-1\""),
                        );
                        resp
                    }
                }),
            )
            .route(
                "/subjects",
                get(move |Query(params): Query<HashMap<String, String>>| {
                    let base = base_for_subj.clone();
                    async move {
                        if let Some(ids) = params.get("ids") {
                            let data: Vec<serde_json::Value> = ids
                                .split(',')
                                .filter_map(|s| s.parse::<i64>().ok())
                                .map(|id| {
                                    json!({
                                        "id": id,
                                        "data": { "characters": "新", "level": 25 }
                                    })
                                })
                                .collect();
                            return Json(json!({
                                "data_updated_at": "2026-11-25T00:10:00Z",
                                "pages": { "next_url": null },
                                "data": data
                            }));
                        }

                        let page_num: usize = params
                            .get("page")
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(1);
                        let total_pages = 23usize;
                        let next_url = if page_num < total_pages {
                            Some(format!("{}/subjects?page={}", base, page_num + 1))
                        } else {
                            None
                        };
                        let kanji_pool = [
                            "一","二","三","四","五","六","七","八","九","十",
                            "日","月","火","水","木","金","土","山","川","田",
                            "人","口","目",
                        ];
                        Json(json!({
                            "data_updated_at": "2026-11-25T00:00:00Z",
                            "pages": { "next_url": next_url },
                            "data": [{
                                "id": page_num as i64,
                                "data": {
                                    "characters": kanji_pool[(page_num - 1) % kanji_pool.len()],
                                    "level": page_num as i32
                                }
                            }]
                        }))
                    }
                }),
            );

        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = WaniKaniClient::new(base_url, 5).unwrap();

        // 1. Full sync fetches all 23 pages & recovers from HTTP 429
        let mut progress_events = 0usize;
        let full = client
            .sync_with_progress("test-token", &WaniKaniSyncCursor::default(), true, |_| {
                progress_events += 1;
            })
            .await
            .unwrap();
        assert!(!full.is_incremental);
        assert_eq!(full.username, "kanji_master");
        assert_eq!(full.level, 25);
        assert_eq!(full.lessons_available, 3);
        assert_eq!(full.reviews_available, 2);
        assert_eq!(full.subjects.len(), 23, "must fetch beyond old 20-page cap");
        assert_eq!(rate_limit_hits.load(Ordering::SeqCst), 3);
        assert!(progress_events > 20);

        // 2. Incremental sync uses updated_after & ETag (304 on /user & /summary)
        let cursor = WaniKaniSyncCursor {
            user_etag: full.user_etag.clone(),
            summary_etag: full.summary_etag.clone(),
            assignments_updated_after: full.assignments_updated_after,
            assignments_etag: full.assignments_etag.clone(),
            subjects_updated_after: full.subjects_updated_after,
            subjects_etag: full.subjects_etag.clone(),
            cached_username: Some(full.username.clone()),
            cached_level: full.level,
            cached_lessons: full.lessons_available,
            cached_reviews: full.reviews_available,
        };
        let inc = client
            .sync_with_progress("test-token", &cursor, false, |_| {})
            .await
            .unwrap();
        assert!(inc.is_incremental);
        assert_eq!(inc.username, "kanji_master");
        assert_eq!(inc.subjects.len(), 1);
        assert_eq!(inc.subjects[0].0, 100);
        assert_eq!(inc.removed_subject_ids, vec![2]);
        assert_eq!(updated_after_seen.load(Ordering::SeqCst), 1);

        // 3. Subsequent incremental sync gets 304 Not Modified on /assignments too
        let cursor2 = WaniKaniSyncCursor {
            assignments_etag: inc.assignments_etag.clone(),
            assignments_updated_after: inc.assignments_updated_after,
            ..cursor
        };
        let inc_304 = client
            .sync_with_progress("test-token", &cursor2, false, |_| {})
            .await
            .unwrap();
        assert!(inc_304.is_incremental);
        assert!(inc_304.subjects.is_empty());
        assert!(inc_304.removed_subject_ids.is_empty());

        server.abort();
    }
}
