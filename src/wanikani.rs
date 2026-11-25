use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const WK_API_BASE: &str = "https://api.wanikani.com/v2";

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

pub struct WaniKaniSyncResult {
    pub username: String,
    pub level: i32,
    pub lessons_available: i32,
    pub reviews_available: i32,
    pub subjects: Vec<(i64, String, String, i32, i32)>, // (subject_id, type, characters, level, srs_stage)
}

/// Real WaniKani API v2 client using Bearer authentication.
pub async fn sync_from_wanikani(api_token: &str) -> Result<WaniKaniSyncResult> {
    let client = reqwest::Client::builder()
        .user_agent("BunAmateur/0.1 (Japanese Grammar SRS)")
        .build()?;

    let auth = format!("Bearer {}", api_token.trim());

    // 1. Fetch User Profile
    let user_resp = client
        .get(format!("{}/user", WK_API_BASE))
        .header("Authorization", &auth)
        .header("Wanikani-Revision", "20170710")
        .send()
        .await
        .context("Failed to reach WaniKani API")?;

    if !user_resp.status().is_success() {
        let code = user_resp.status();
        return Err(anyhow!(
            "WaniKani authentication failed (HTTP {}). Check your Personal Access Token.",
            code
        ));
    }

    let user: WkUserEnvelope = user_resp.json().await?;
    let username = user.data.username;
    let level = user.data.level;

    // 2. Fetch Summary (available lessons & reviews now)
    let summary_resp = client
        .get(format!("{}/summary", WK_API_BASE))
        .header("Authorization", &auth)
        .header("Wanikani-Revision", "20170710")
        .send()
        .await?;

    let (lessons_available, reviews_available) = if summary_resp.status().is_success() {
        let summary: WkSummaryEnvelope = summary_resp.json().await?;
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
        (lessons, reviews)
    } else {
        (0, 0)
    };

    // 3. Fetch passed assignments (Guru+ kanji and vocabulary) with pagination
    let mut passed_map: HashMap<i64, (String, i32)> = HashMap::new();
    let mut next_url = Some(format!(
        "{}/assignments?subject_types=kanji,vocabulary&passed=true",
        WK_API_BASE
    ));
    let mut page_guard = 0;

    while let Some(url) = next_url {
        page_guard += 1;
        if page_guard > 20 {
            break;
        }
        let resp = client
            .get(&url)
            .header("Authorization", &auth)
            .header("Wanikani-Revision", "20170710")
            .send()
            .await?;
        if !resp.status().is_success() {
            break;
        }
        let page: WkCollectionEnvelope<WkAssignmentData> = resp.json().await?;
        for item in page.data {
            if item.data.passed_at.is_some() || item.data.srs_stage >= 5 {
                passed_map.insert(
                    item.data.subject_id,
                    (item.data.subject_type, item.data.srs_stage),
                );
            }
        }
        next_url = page.pages.next_url;
    }

    // 4. Fetch subjects up to the user's current level
    let levels_param: String = (1..=level.clamp(1, 60))
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let mut subjects_out = Vec::new();
    let mut subj_url = Some(format!(
        "{}/subjects?types=kanji,vocabulary&levels={}",
        WK_API_BASE, levels_param
    ));
    let mut subj_guard = 0;

    while let Some(url) = subj_url {
        subj_guard += 1;
        if subj_guard > 20 {
            break;
        }
        let resp = client
            .get(&url)
            .header("Authorization", &auth)
            .header("Wanikani-Revision", "20170710")
            .send()
            .await?;
        if !resp.status().is_success() {
            break;
        }
        let page: WkCollectionEnvelope<WkSubjectData> = resp.json().await?;
        for item in page.data {
            if let Some((stype, srs)) = passed_map.get(&item.id)
                && let Some(chars) = item.data.characters
            {
                subjects_out.push((item.id, stype.clone(), chars, item.data.level, *srs));
            }
        }
        subj_url = page.pages.next_url;
    }

    Ok(WaniKaniSyncResult {
        username,
        level,
        lessons_available,
        reviews_available,
        subjects: subjects_out,
    })
}

/// Demo WaniKani Level 14 sync so users can experience Smart Furigana immediately without an API token.
pub fn generate_demo_wanikani_sync() -> WaniKaniSyncResult {
    // Authentic WaniKani Levels 1–14 high-frequency kanji
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
