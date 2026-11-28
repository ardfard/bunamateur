//! Pure-Rust Japanese morphological analyzer, deconjugator, automatic furigana
//! annotator, WaniKani enrichment engine, and grammar pattern matcher.
//!
//! Designed to run 100% offline with zero external NLP dependencies by combining
//! a curated embedded JMdict-core lexicon, longest-match morphological
//! segmentation, multi-step verb/adjective deconjugation, and okurigana-aware
//! ruby generation.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// Identifier string reported in [`NlpAnalysis::analyzer_engine`].
pub const ANALYZER_ENGINE_NAME: &str = "pure-rust-morph-jmdict-v1";

/// A single morphologically analyzed Japanese token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalyzedToken {
    /// Exact surface slice from the normalized input text.
    pub surface: String,
    /// Dictionary / base form obtained via lexicon lookup or deconjugation.
    pub lemma: String,
    /// Hiragana reading of `surface`.
    pub reading: String,
    /// Human-readable part-of-speech tag: `"noun"`, `"verb"`, `"i-adj"`,
    /// `"na-adj"`, `"adverb"`, `"particle"`, `"auxiliary"`, `"conjunction"`,
    /// `"expression"`, `"pronoun"`, `"number"`, `"punctuation"`, or `"other"`.
    pub pos: String,
    /// English glosses from the embedded JMdict-core lexicon (empty if unknown/punctuation).
    pub glosses: Vec<String>,
    /// JLPT level tag (e.g. `Some("N5")`..`Some("N1")`) when known.
    pub jlpt: Option<String>,
    /// Auto-furigana markup for this token (e.g. `"[毎日|まいにち]"` or `"[食|た]べる"`).
    pub ruby: String,
    /// WaniKani level populated from [`WkSubjectLookup`] when available.
    pub wk_level: Option<i32>,
    /// `true` if the token contains kanji and all kanji in the token are known in WaniKani.
    pub wk_known: bool,
    /// `true` for clickable dictionary tokens with glosses or content POS / kanji.
    pub is_content_word: bool,
}

/// WaniKani subject lookup table used to enrich analyzed tokens.
#[derive(Debug, Clone, Default)]
pub struct WkSubjectLookup {
    /// Single kanji character -> WaniKani level.
    pub kanji_levels: HashMap<String, i32>,
    /// Vocabulary surface/lemma -> WaniKani level.
    pub vocab_levels: HashMap<String, i32>,
    /// Kanji characters already known by the user.
    pub known_kanji: HashSet<String>,
}

/// Complete morphological analysis output for a Japanese text passage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NlpAnalysis {
    pub tokens: Vec<AnalyzedToken>,
    /// Full text reconstructed with automatic `[漢字|かんじ]` furigana.
    pub annotated_ruby: String,
    pub analyzer_engine: String,
}

/// Pluggable Japanese morphological analyzer interface.
pub trait JapaneseAnalyzer: Send + Sync {
    fn engine_name(&self) -> &'static str;
    fn analyze(&self, text: &str, wk: &WkSubjectLookup) -> NlpAnalysis;
}

/// Pure-Rust analyzer backed by an embedded JMdict-core lexicon and rule-based deconjugator.
#[derive(Debug, Default, Clone, Copy)]
pub struct EmbeddedJapaneseAnalyzer;

impl JapaneseAnalyzer for EmbeddedJapaneseAnalyzer {
    fn engine_name(&self) -> &'static str {
        ANALYZER_ENGINE_NAME
    }

    fn analyze(&self, text: &str, wk: &WkSubjectLookup) -> NlpAnalysis {
        analyze_japanese_text(text, wk)
    }
}

/// Strip `[漢字|かんじ]` furigana markup down to plain text (`漢字`).
pub fn strip_ruby_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'[' {
            let rest = &input[i + 1..];
            if let Some(bar_rel) = rest.find('|') {
                let base = &rest[..bar_rel];
                let after_bar = &rest[bar_rel + 1..];
                if !base.contains('[')
                    && !base.contains(']')
                    && !base.contains('\n')
                    && let Some(close_rel) = after_bar.find(']')
                {
                    let reading = &after_bar[..close_rel];
                    if !reading.contains('[') && !reading.contains('\n') {
                        out.push_str(base);
                        i += 1 + bar_rel + 1 + close_rel + 1;
                        continue;
                    }
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

/// Analyze Japanese text into tokens, automatic furigana, and WaniKani enrichment.
pub fn analyze_japanese_text(text: &str, wk: &WkSubjectLookup) -> NlpAnalysis {
    let plain = strip_ruby_text(text);
    let lex = lexicon();
    let char_offsets: Vec<usize> = plain
        .char_indices()
        .map(|(idx, _)| idx)
        .chain(std::iter::once(plain.len()))
        .collect();
    let total_chars = char_offsets.len().saturating_sub(1);

    let mut tokens = Vec::new();
    let mut pos = 0usize;

    while pos < total_chars {
        let start_byte = char_offsets[pos];
        let first_ch = plain[start_byte..].chars().next().unwrap();

        // 1. Punctuation or whitespace
        if is_jp_punctuation(first_ch) {
            let mut end = pos + 1;
            if first_ch.is_ascii_whitespace() {
                while end < total_chars {
                    let ch = plain[char_offsets[end]..].chars().next().unwrap();
                    if ch.is_ascii_whitespace() {
                        end += 1;
                    } else {
                        break;
                    }
                }
            }
            let surface = &plain[start_byte..char_offsets[end]];
            tokens.push(build_plain_token(surface, "", "punctuation", false, wk));
            pos = end;
            continue;
        }

        // 2. Numeric literals (ASCII or full-width digits)
        if is_digit_char(first_ch) {
            let mut end = pos + 1;
            while end < total_chars {
                let ch = plain[char_offsets[end]..].chars().next().unwrap();
                if is_digit_char(ch) {
                    end += 1;
                } else {
                    break;
                }
            }
            let surface = &plain[start_byte..char_offsets[end]];
            tokens.push(build_plain_token(surface, surface, "number", true, wk));
            pos = end;
            continue;
        }

        // 3. Longest-match lexicon + morphological deconjugation search
        let max_len = (total_chars - pos).min(16);
        let mut matched: Option<(usize, AnalyzedToken)> = None;

        for len in (1..=max_len).rev() {
            let end_byte = char_offsets[pos + len];
            let slice = &plain[start_byte..end_byte];

            // Skip spans containing punctuation/whitespace
            if slice.chars().any(is_jp_punctuation) {
                continue;
            }

            // Priority A: Exact lexicon match
            if let Some(entries) = lex.by_surface.get(slice) {
                let entry = entries[0];
                let lemma = entry.lemma.unwrap_or(entry.surface);
                let token = build_lex_token(slice, lemma, entry.reading, entry, None, wk);
                matched = Some((pos + len, token));
                break;
            }

            // Priority B: Inflected verb / i-adjective deconjugation
            let next_text = &plain[end_byte..];
            if let Some(dm) = deconjugate_for_segmenter(slice, next_text, lex) {
                let lemma = dm.entry.lemma.unwrap_or(dm.entry.surface);
                let token = build_lex_token(
                    slice,
                    lemma,
                    &dm.derived_reading,
                    dm.entry,
                    Some(dm.label),
                    wk,
                );
                matched = Some((pos + len, token));
                break;
            }
        }

        if let Some((next_pos, token)) = matched {
            tokens.push(token);
            pos = next_pos;
            continue;
        }

        // 4. Unknown token fallback (Latin, Katakana, Kanji, or single Hiragana)
        if first_ch.is_ascii_alphabetic() {
            let mut end = pos + 1;
            while end < total_chars {
                let ch = plain[char_offsets[end]..].chars().next().unwrap();
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    end += 1;
                } else {
                    break;
                }
            }
            let surface = &plain[start_byte..char_offsets[end]];
            tokens.push(build_plain_token(surface, surface, "other", false, wk));
            pos = end;
            continue;
        }

        if is_katakana(first_ch) {
            let mut end = pos + 1;
            while end < total_chars {
                let b = char_offsets[end];
                let ch = plain[b..].chars().next().unwrap();
                if !is_katakana(ch) || has_known_prefix(&plain[b..], lex) {
                    break;
                }
                end += 1;
            }
            let surface = &plain[start_byte..char_offsets[end]];
            let reading = katakana_to_hiragana(surface);
            tokens.push(build_plain_token(surface, &reading, "noun", true, wk));
            pos = end;
            continue;
        }

        if is_kanji(first_ch) {
            let mut end = pos + 1;
            while end < total_chars {
                let b = char_offsets[end];
                let ch = plain[b..].chars().next().unwrap();
                if !is_kanji(ch) || has_known_prefix(&plain[b..], lex) {
                    break;
                }
                end += 1;
            }
            let surface = &plain[start_byte..char_offsets[end]];
            tokens.push(build_plain_token(surface, "", "noun", true, wk));
            pos = end;
            continue;
        }

        // Single unknown hiragana / symbol: advance 1 char so we resync immediately
        let end = pos + 1;
        let surface = &plain[start_byte..char_offsets[end]];
        let reading = katakana_to_hiragana(surface);
        tokens.push(build_plain_token(surface, &reading, "other", false, wk));
        pos = end;
    }

    let annotated_ruby: String = tokens.iter().map(|t| t.ruby.as_str()).collect();

    NlpAnalysis {
        tokens,
        annotated_ruby,
        analyzer_engine: ANALYZER_ENGINE_NAME.to_string(),
    }
}

/// Automatically annotate plain (or partially annotated) Japanese text with `[漢字|かんじ]` furigana.
pub fn auto_annotate_ruby(text: &str) -> String {
    let empty_wk = WkSubjectLookup::default();
    analyze_japanese_text(text, &empty_wk).annotated_ruby
}

/// Deconjugate an inflected Japanese verb or adjective surface form into candidate
/// `(dictionary_lemma, conjugation_label)` pairs, prioritizing lexicon-verified entries.
pub fn deconjugate_surface(surface: &str) -> Vec<(String, &'static str)> {
    let clean = strip_ruby_text(surface).trim().to_string();
    if clean.is_empty() {
        return Vec::new();
    }

    let lex = lexicon();
    let mut verified: Vec<(String, &'static str)> = Vec::new();
    let mut unverified: Vec<(String, &'static str)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Scope the mutable borrow of `verified`/`seen`/`unverified` so the fallback
    // merge below can mutate them freely (clippy::drop_non_drop on Rust 1.92).
    {
    let mut push_candidate =
        |lemma: String, label: &'static str, allowed: TargetClass, force_verified: bool| {
            if lemma.is_empty() || lemma == clean {
                return;
            }
            let is_verified = force_verified || is_in_lexicon_with_class(&lemma, allowed, lex);
            if is_verified {
                if seen.insert(lemma.clone()) {
                    verified.push((lemma, label));
                }
            } else {
                unverified.push((lemma, label));
            }
        };

    // 1. Compound suru-verb progressive/auxiliary forms (e.g., 勉強している -> 勉強する)
    const SURU_COMPOUND_RULES: &[(&str, &str)] = &[
        ("していませんでした", "progressive-polite-negative-past"),
        ("していません", "progressive-polite-negative"),
        ("していました", "progressive-polite-past"),
        ("しています", "progressive-polite"),
        ("していなかった", "progressive-negative-past"),
        ("していない", "progressive-negative"),
        ("していた", "progressive-past"),
        ("している", "progressive"),
        ("しなければなりません", "obligation-polite"),
        ("しなければならない", "obligation"),
        ("しませんでした", "polite-negative-past"),
        ("しません", "polite-negative"),
        ("しました", "polite-past"),
        ("します", "polite"),
        ("しましょう", "polite-volitional"),
        ("しなかった", "negative-past"),
        ("しなければ", "negative-conditional"),
        ("しない", "negative"),
        ("したかった", "desiderative-past"),
        ("したくない", "desiderative-negative"),
        ("したい", "desiderative"),
        ("して", "te-form"),
        ("した", "past"),
        ("しよう", "volitional"),
        ("すれば", "conditional"),
        ("される", "passive"),
        ("された", "passive-past"),
        ("させる", "causative"),
        ("させた", "causative-past"),
        ("できる", "potential"),
        ("できた", "potential-past"),
        ("できます", "potential-polite"),
    ];

    for &(suffix, label) in SURU_COMPOUND_RULES {
        if let Some(stem) = clean.strip_suffix(suffix) {
            if stem.is_empty() {
                push_candidate("する".to_string(), label, TargetClass::Suru, false);
            } else if is_suru_noun(stem, lex) {
                push_candidate(format!("{stem}する"), label, TargetClass::Suru, true);
            } else if stem.chars().any(is_kanji) {
                push_candidate(format!("{stem}する"), label, TargetClass::Suru, false);
            }
        }
    }

    // 2. Verb + progressive/auxiliary compound endings (e.g. 食べている -> 食べる, 読んでいます -> 読む)
    const TE_AUX_TAILS: &[(&str, &str)] = &[
        ("ていませんでした", "progressive-polite-negative-past"),
        ("ていません", "progressive-polite-negative"),
        ("ていました", "progressive-polite-past"),
        ("ています", "progressive-polite"),
        ("ていなかった", "progressive-negative-past"),
        ("ていない", "progressive-negative"),
        ("ていた", "progressive-past"),
        ("ている", "progressive"),
        ("てる", "progressive-casual"),
        ("てしまいました", "completive-polite-past"),
        ("てしまった", "completive-past"),
        ("てしまう", "completive"),
        ("ておきます", "preparatory-polite"),
        ("ておく", "preparatory"),
        ("てみます", "experiential-polite"),
        ("てみる", "experiential"),
        ("でいません", "progressive-polite-negative"),
        ("でいました", "progressive-polite-past"),
        ("でいます", "progressive-polite"),
        ("でいない", "progressive-negative"),
        ("でいた", "progressive-past"),
        ("でいる", "progressive"),
    ];

    for &(tail, label) in TE_AUX_TAILS {
        if let Some(prefix) = clean.strip_suffix(tail) {
            let te_char = if tail.starts_with('で') { "で" } else { "て" };
            let te_form = format!("{prefix}{te_char}");
            for (cand, _, cls) in generate_single_step_candidates(&te_form) {
                push_candidate(cand, label, cls, false);
            }
        }
    }

    // 3. Single-step & chained (2-step) verb/adjective deconjugation
    let step1 = generate_single_step_candidates(&clean);
    for (cand, label, cls) in &step1 {
        push_candidate(cand.clone(), label, *cls, false);
    }

    // Chained step 2 (e.g., 読めなかった -> 読める -> 読む, 行われた -> 行われる -> 行う)
    for (mid, label1, mid_cls) in &step1 {
        if *mid_cls != TargetClass::Ichidan {
            continue;
        }
        let step2 = generate_secondary_voice_candidates(mid);
        for (cand2, label2, cls2) in step2 {
            let combined = combine_voice_label(label2, label1);
            push_candidate(cand2, combined, cls2, false);
        }
    }

    // 4. な-adjective copula forms (e.g. 静かだった -> 静か)
    const NA_ADJ_SUFFIXES: &[(&str, &str)] = &[
        ("ではありませんでした", "polite-negative-past"),
        ("ではありません", "polite-negative"),
        ("じゃありません", "polite-negative"),
        ("ではなかった", "negative-past"),
        ("じゃなかった", "negative-past"),
        ("ではない", "negative"),
        ("じゃない", "negative"),
        ("でした", "polite-past"),
        ("だった", "past"),
        ("です", "polite"),
        ("だ", "plain"),
    ];

    for &(suffix, label) in NA_ADJ_SUFFIXES {
        if let Some(stem) = clean.strip_suffix(suffix)
            && !stem.is_empty()
        {
            push_candidate(stem.to_string(), label, TargetClass::NaAdj, false);
        }
    }
    } // end push_candidate borrow scope

    // Append deduplicated unverified fallbacks only if not already in verified
    for (lemma, label) in unverified {
        if seen.insert(lemma.clone()) {
            verified.push((lemma, label));
        }
    }

    verified
}

/// Match a grammar `pattern` (e.g. `"ている"`, `"ようになる"`, `"おかげで"`,
/// `"にもかかわらず"`, `"なければならない"`, `"つもり"`) against a morphologically
/// analyzed Japanese text, returning the matched surface snippet if found.
///
/// Uses token surfaces, lemmas, readings, verb inflectional suffixes, and
/// contiguous multi-token spans while avoiding false positives inside unrelated
/// words (e.g. `"たい"` inside `大変` or `痛い`, `"のに"` inside `昨日の荷物`).
pub fn match_pattern_against_analysis(
    pattern: &str,
    plain_text: &str,
    analysis: &NlpAnalysis,
) -> Option<String> {
    let cleaned_raw = strip_ruby_text(pattern);
    let cleaned = cleaned_raw
        .trim()
        .trim_matches(|c| matches!(c, '〜' | '～' | '~' | ' '))
        .trim();
    if cleaned.is_empty() {
        return None;
    }

    let pat_chars = cleaned.chars().count();
    let pat_norm = normalize_grammar_form(cleaned);
    let tokens = &analysis.tokens;

    // Strategy 1: Single-token exact surface, lemma, reading (len >= 2), or verb inflectional suffix
    for tok in tokens {
        if tok.pos == "punctuation" {
            continue;
        }
        if tok.surface == cleaned || tok.lemma == cleaned {
            return Some(tok.surface.clone());
        }
        if pat_chars >= 2 && !tok.reading.is_empty() && tok.reading == cleaned {
            return Some(tok.surface.clone());
        }
        if pat_chars >= 2 && normalize_grammar_form(&tok.surface) == pat_norm {
            return Some(tok.surface.clone());
        }

        // Inflected verb suffix matching (e.g. 行きたい -> たい, 食べたくなかった -> たい)
        if tok.pos == "verb"
            && tok.surface != tok.lemma
            && let Some(suffix) = extract_verb_inflection_suffix(&tok.surface, &tok.lemma)
            && !suffix.is_empty()
            && (suffix == cleaned
                || normalize_grammar_form(suffix) == pat_norm
                || (cleaned == "たい" && is_desiderative_suffix(suffix)))
        {
            return Some(suffix.to_string());
        }
    }

    // Strategy 2: Contiguous multi-token spans (1..=6 tokens)
    let max_span = 6usize;
    for i in 0..tokens.len() {
        if tokens[i].pos == "punctuation" {
            continue;
        }

        let mut span_surface = String::new();
        let mut span_reading = String::new();

        for j in i..(i + max_span).min(tokens.len()) {
            let t = &tokens[j];
            if t.pos == "punctuation" {
                break;
            }
            let prefix_surface_len = span_surface.len();
            span_surface.push_str(&t.surface);
            if t.reading.is_empty() {
                span_reading.push_str(&t.surface);
            } else {
                span_reading.push_str(&t.reading);
            }

            // Lemma-tail form: surface of tokens[i..j] + lemma of tokens[j]
            let mut span_lemma_tail = String::with_capacity(prefix_surface_len + t.lemma.len());
            span_lemma_tail.push_str(&span_surface[..prefix_surface_len]);
            span_lemma_tail.push_str(&t.lemma);

            let span_norm = normalize_grammar_form(&span_surface);
            let lemma_tail_norm = normalize_grammar_form(&span_lemma_tail);

            // Direct span equality
            if span_surface == cleaned
                || span_lemma_tail == cleaned
                || span_norm == pat_norm
                || lemma_tail_norm == pat_norm
                || (pat_chars >= 2 && span_reading == cleaned)
            {
                return Some(span_surface);
            }

            // First-token verb inflectional suffix + rest of span
            // Examples:
            //   [食べて, いる]           -> "て" + "いる" = "ている"
            //   [食べて, いました]        -> "て" + lemma("いる") = "ている"
            //   [飲んで, いる]           -> "で" + "いる" matches "ている"
            //   [行かなければ, なりません] -> "なければ" + norm("ならない") = "なければならない"
            //   [登った, こと, が, あります] -> "た" + "ことがある" = "たことがある"
            if j > i
                && tokens[i].pos == "verb"
                && let Some(first_suffix) =
                    extract_verb_inflection_suffix(&tokens[i].surface, &tokens[i].lemma)
            {
                let rest_surface = &span_surface[tokens[i].surface.len()..];
                let rest_lemma_tail = &span_lemma_tail[tokens[i].surface.len()..];

                let joined_surface = format!("{first_suffix}{rest_surface}");
                let joined_lemma = format!("{first_suffix}{rest_lemma_tail}");

                if matches_grammar_tail(&joined_surface, &joined_lemma, cleaned, &pat_norm) {
                    return Some(joined_surface);
                }

                // Also test canonical trailing te/de/ta/da/nakereba boundaries on the first token
                for &boundary in &["なければ", "て", "で", "た", "だ", "たい", "たく", "たかっ"]
                {
                    if first_suffix.ends_with(boundary) {
                        let b_surface = format!("{boundary}{rest_surface}");
                        let b_lemma = format!("{boundary}{rest_lemma_tail}");
                        if matches_grammar_tail(&b_surface, &b_lemma, cleaned, &pat_norm) {
                            return Some(b_surface);
                        }
                    }
                }
            }
        }
    }

    // Strategy 3: Safe plain-text substring fallback ONLY for long/unambiguous patterns
    // (>= 4 chars, or >= 3 chars containing kanji) so short patterns never false-match.
    let has_kanji = cleaned.chars().any(is_kanji);
    if (pat_chars >= 4 || (pat_chars >= 3 && has_kanji)) && plain_text.contains(cleaned) {
        return Some(cleaned.to_string());
    }

    None
}

fn matches_grammar_tail(
    joined_surface: &str,
    joined_lemma: &str,
    cleaned_pattern: &str,
    pat_norm: &str,
) -> bool {
    if joined_surface == cleaned_pattern || joined_lemma == cleaned_pattern {
        return true;
    }
    let norm_surf = normalize_grammar_form(joined_surface);
    let norm_lemma = normalize_grammar_form(joined_lemma);
    if norm_surf == pat_norm || norm_lemma == pat_norm {
        return true;
    }
    // Voiced て/で and た/だ equivalence (e.g., 飲んでいる matches ている, 読んだことがある matches たことがある)
    if let Some(rest) = cleaned_pattern.strip_prefix('て') {
        let voiced = format!("で{rest}");
        let voiced_norm = normalize_grammar_form(&voiced);
        if joined_surface == voiced
            || joined_lemma == voiced
            || norm_surf == voiced_norm
            || norm_lemma == voiced_norm
        {
            return true;
        }
    }
    if let Some(rest) = cleaned_pattern.strip_prefix('た') {
        let voiced = format!("だ{rest}");
        let voiced_norm = normalize_grammar_form(&voiced);
        if joined_surface == voiced
            || joined_lemma == voiced
            || norm_surf == voiced_norm
            || norm_lemma == voiced_norm
        {
            return true;
        }
    }
    false
}

/// Normalize polite/tense/kanji variations of grammar expressions to a canonical plain form.
fn normalize_grammar_form(s: &str) -> String {
    // Check longest suffixes first
    const REPLACEMENTS: &[(&str, &str)] = &[
        ("なければなりませんでした", "なければならない"),
        ("なければなりません", "なければならない"),
        ("なければならなかった", "なければならない"),
        ("なくてはなりません", "なければならない"),
        ("なくてはいけません", "なければならない"),
        ("なくてはいけない", "なければならない"),
        ("わけにはいきません", "わけにはいかない"),
        ("訳にはいきません", "わけにはいかない"),
        ("訳にはいかない", "わけにはいかない"),
        ("わけではありません", "わけではない"),
        ("訳ではありません", "わけではない"),
        ("訳ではない", "わけではない"),
        ("わけじゃない", "わけではない"),
        ("にも関わらず", "にもかかわらず"),
        ("に違いありません", "に違いない"),
        ("にちがいありません", "に違いない"),
        ("にちがいない", "に違いない"),
        ("ざるを得ません", "ざるを得ない"),
        ("ざるをえません", "ざるを得ない"),
        ("ざるをえない", "ざるを得ない"),
        ("に堪えません", "に堪えない"),
        ("にたえません", "に堪えない"),
        ("にたえない", "に堪えない"),
        ("にたえる", "に堪える"),
        ("お陰で", "おかげで"),
        ("代わりに", "かわりに"),
        ("内に", "うちに"),
        ("方がいいです", "ほうがいい"),
        ("ほうがいいです", "ほうがいい"),
        ("方がいい", "ほうがいい"),
        ("ようになりました", "ようになる"),
        ("ようになった", "ようになる"),
        ("ようになります", "ようになる"),
        ("ようにしています", "ようにする"),
        ("ようにしている", "ようにする"),
        ("ようにしました", "ようにする"),
        ("ようにします", "ようにする"),
        ("ことにしました", "ことにする"),
        ("ことにした", "ことにする"),
        ("ことにしています", "ことにする"),
        ("ことにしている", "ことにする"),
        ("ことにします", "ことにする"),
        ("てしまいました", "てしまう"),
        ("てしまった", "てしまう"),
        ("てしまいます", "てしまう"),
        ("でしまいました", "でしまう"),
        ("でしまった", "でしまう"),
        ("ていました", "ている"),
        ("ていません", "ている"),
        ("ています", "ている"),
        ("ていた", "ている"),
        ("ていない", "ている"),
        ("でいました", "でいる"),
        ("でいません", "でいる"),
        ("でいます", "でいる"),
        ("でいた", "でいる"),
        ("ておきました", "ておく"),
        ("ておきます", "ておく"),
        ("ておいた", "ておく"),
        ("てみました", "てみる"),
        ("てみます", "てみる"),
        ("てみた", "てみる"),
        ("たことがあります", "たことがある"),
        ("たことがありません", "たことがある"),
        ("たことがない", "たことがある"),
        ("てもいいです", "てもいい"),
        ("てもいいですか", "てもいい"),
        ("てもよろしいですか", "てもいい"),
        ("つもりでした", "つもりだ"),
        ("つもりだった", "つもりだ"),
        ("つもりです", "つもりだ"),
        ("はずでした", "はずだ"),
        ("はずだった", "はずだ"),
        ("はずです", "はずだ"),
        ("一方です", "一方だ"),
        ("いっぽうです", "一方だ"),
        ("いっぽうだ", "一方だ"),
        ("おかげです", "おかげで"),
        ("おかげだ", "おかげで"),
        ("せいです", "せいで"),
        ("せいだ", "せいで"),
        ("なりませんでした", "ならない"),
        ("なりません", "ならない"),
        ("ならなかった", "ならない"),
        ("ありませんでした", "ない"),
        ("ありません", "ない"),
        ("いきません", "いかない"),
        ("いませんでした", "いる"),
        ("いません", "いる"),
        ("いました", "いる"),
        ("います", "いる"),
        ("なりました", "なる"),
        ("なります", "なる"),
        ("なった", "なる"),
        ("しまいました", "しまう"),
        ("しまいます", "しまう"),
        ("しまった", "しまう"),
        ("おきました", "おく"),
        ("おきます", "おく"),
        ("おいた", "おく"),
        ("みました", "みる"),
        ("みます", "みる"),
        ("みた", "みる"),
        ("しました", "する"),
        ("します", "する"),
        ("した", "する"),
        ("たくなかった", "たい"),
        ("たくない", "たい"),
        ("たかった", "たい"),
        ("たいです", "たい"),
        ("でした", "だ"),
        ("だった", "だ"),
        ("です", "だ"),
    ];

    for &(from, to) in REPLACEMENTS {
        if let Some(prefix) = s.strip_suffix(from) {
            return format!("{prefix}{to}");
        }
    }
    s.to_string()
}

fn is_desiderative_suffix(suffix: &str) -> bool {
    matches!(
        suffix,
        "たい" | "たくない" | "たかった" | "たくなかった" | "たく" | "たいです"
    ) || suffix.ends_with("たい")
        || suffix.ends_with("たくない")
        || suffix.ends_with("たかった")
        || suffix.ends_with("たくなかった")
}

/// Extract the inflectional kana suffix of an inflected verb relative to its lemma.
/// Example: `("食べて", "食べる")` -> `Some("て")`,
///          `("行かなければ", "行く")` -> `Some("かなければ")`,
///          `("して", "する")` -> `Some("て")`.
fn extract_verb_inflection_suffix<'a>(surface: &'a str, lemma: &str) -> Option<&'a str> {
    if surface == lemma {
        return None;
    }
    if lemma == "する" {
        for &suf in &[
            "しなければ",
            "しませんでした",
            "しません",
            "しました",
            "します",
            "しなかった",
            "しない",
            "したかった",
            "したくない",
            "したい",
            "して",
            "した",
        ] {
            if surface == suf {
                // For して/した, return "て"/"た" so して + いる matches ている!
                return Some(suf.strip_prefix('し').unwrap_or(suf));
            }
        }
        return Some(surface);
    }

    let mut s_chars = surface.char_indices().peekable();
    let mut l_chars = lemma.chars().peekable();
    let mut common_byte = 0usize;

    while let (Some(&(idx, sc)), Some(&lc)) = (s_chars.peek(), l_chars.peek()) {
        if sc == lc {
            common_byte = idx + sc.len_utf8();
            s_chars.next();
            l_chars.next();
        } else {
            break;
        }
    }

    if common_byte < surface.len() {
        Some(&surface[common_byte..])
    } else {
        None
    }
}

// ============================================================================
// Character Classification & Okurigana-Aware Ruby Formatter
// ============================================================================

fn is_kanji(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c) || ('\u{3400}'..='\u{4DBF}').contains(&c) || c == '々'
}

fn is_hiragana(c: char) -> bool {
    ('\u{3040}'..='\u{309F}').contains(&c)
}

fn is_katakana(c: char) -> bool {
    ('\u{30A0}'..='\u{30FF}').contains(&c)
}

fn is_kana(c: char) -> bool {
    is_hiragana(c) || is_katakana(c)
}

fn is_digit_char(c: char) -> bool {
    c.is_ascii_digit() || ('０'..='９').contains(&c)
}

fn is_jp_punctuation(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '。' | '、'
                | '！'
                | '？'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '・'
                | '…'
                | '〜'
                | '～'
                | '：'
                | '；'
                | '【'
                | '】'
                | '《'
                | '》'
                | '〈'
                | '〉'
                | '—'
                | '―'
                | '.'
                | ','
                | '!'
                | '?'
                | '('
                | ')'
                | '['
                | ']'
                | ':'
                | ';'
                | '"'
                | '\''
                | '/'
                | '　'
        )
}

fn kana_char_to_hiragana(c: char) -> char {
    if ('\u{30A1}'..='\u{30F6}').contains(&c) {
        char::from_u32(c as u32 - 0x60).unwrap_or(c)
    } else {
        c
    }
}

fn katakana_to_hiragana(s: &str) -> String {
    s.chars().map(kana_char_to_hiragana).collect()
}

/// Format okurigana-aware `[漢字|かんじ]` ruby markup.
///
/// Examples:
/// - `("食べる", "たべる")` -> `"[食|た]べる"`
/// - `("新聞", "しんぶん")` -> `"[新聞|しんぶん]"`
/// - `("少しずつ", "すこしずつ")` -> `"[少|すこ]しずつ"`
/// - `("お茶", "おちゃ")` -> `"お[茶|ちゃ]"`
/// - `("乗り換え", "のりかえ")` -> `"[乗|の]り[換|か]え"`
/// - `("美しい", "うつくしい")` -> `"[美|うつく]しい"`
fn format_ruby(surface: &str, reading: &str) -> String {
    if reading.is_empty() || !surface.chars().any(is_kanji) {
        return surface.to_string();
    }

    let s_chars: Vec<char> = surface.chars().collect();
    let r_chars: Vec<char> = reading.chars().map(kana_char_to_hiragana).collect();

    // Peel matching leading kana (e.g., お in お茶 / お菓子)
    let mut s_start = 0usize;
    let mut r_start = 0usize;
    while s_start < s_chars.len()
        && r_start < r_chars.len()
        && is_kana(s_chars[s_start])
        && kana_char_to_hiragana(s_chars[s_start]) == r_chars[r_start]
    {
        s_start += 1;
        r_start += 1;
    }

    // Peel matching trailing kana (okurigana, e.g., べる in 食べる, しずつ in 少しずつ)
    let mut s_end = s_chars.len();
    let mut r_end = r_chars.len();
    while s_end > s_start
        && r_end > r_start
        && is_kana(s_chars[s_end - 1])
        && kana_char_to_hiragana(s_chars[s_end - 1]) == r_chars[r_end - 1]
    {
        let remaining_kanji = s_chars[s_start..s_end - 1]
            .iter()
            .filter(|&&c| is_kanji(c))
            .count();
        if r_end - 1 - r_start < remaining_kanji.max(1) {
            break;
        }
        s_end -= 1;
        r_end -= 1;
    }

    let prefix: String = s_chars[..s_start].iter().collect();
    let suffix: String = s_chars[s_end..].iter().collect();
    let mid_s = &s_chars[s_start..s_end];
    let mid_r = &r_chars[r_start..r_end];

    if mid_s.is_empty() || mid_r.is_empty() {
        return format!("[{surface}|{reading}]");
    }

    // Handle internal kana between kanji blocks (e.g., 乗り換 / のりか -> [乗|の]り[換|か])
    if mid_s.iter().any(|&c| is_kana(c))
        && let Some(split_mid) = format_internal_kana_ruby(mid_s, mid_r)
    {
        return format!("{prefix}{split_mid}{suffix}");
    }

    let mid_s_str: String = mid_s.iter().collect();
    let mid_r_str: String = mid_r.iter().collect();
    format!("{prefix}[{mid_s_str}|{mid_r_str}]{suffix}")
}

fn format_internal_kana_ruby(mid_s: &[char], mid_r: &[char]) -> Option<String> {
    // Split mid_s into alternating (is_kana, slice) segments
    let mut segments: Vec<(bool, Vec<char>)> = Vec::new();
    for &ch in mid_s {
        let ch_is_kana = is_kana(ch);
        if let Some(last) = segments.last_mut()
            && last.0 == ch_is_kana
        {
            last.1.push(ch);
            continue;
        }
        segments.push((ch_is_kana, vec![ch]));
    }

    // Only attempt alignment when we have at least Kanji + Kana + Kanji
    if segments.len() < 3 {
        return None;
    }

    let mut out = String::new();
    let mut r_pos = 0usize;

    let mut seg_idx = 0usize;
    while seg_idx < segments.len() {
        let (seg_is_kana, ref chars) = segments[seg_idx];
        if seg_is_kana {
            let kana_hira: Vec<char> = chars.iter().map(|&c| kana_char_to_hiragana(c)).collect();
            if r_pos + kana_hira.len() > mid_r.len()
                || mid_r[r_pos..r_pos + kana_hira.len()] != kana_hira[..]
            {
                return None;
            }
            for &c in chars {
                out.push(c);
            }
            r_pos += kana_hira.len();
            seg_idx += 1;
        } else if seg_idx + 1 < segments.len() {
            // Look ahead to the next kana separator
            let next_kana: Vec<char> = segments[seg_idx + 1]
                .1
                .iter()
                .map(|&c| kana_char_to_hiragana(c))
                .collect();
            let remaining_min_tail: usize = segments[seg_idx + 2..]
                .iter()
                .map(|(_, v)| v.len().min(1))
                .sum();

            let search_start = r_pos + 1;
            let search_end = mid_r.len().saturating_sub(next_kana.len() + remaining_min_tail);
            if search_start > search_end {
                return None;
            }

            let found = (search_start..=search_end)
                .find(|&pos| mid_r[pos..pos + next_kana.len()] == next_kana[..])?;

            let kanji_str: String = chars.iter().collect();
            let reading_str: String = mid_r[r_pos..found].iter().collect();
            out.push_str(&format!("[{kanji_str}|{reading_str}]"));
            r_pos = found;
            seg_idx += 1;
        } else {
            // Final kanji segment consumes the rest of mid_r
            if r_pos >= mid_r.len() {
                return None;
            }
            let kanji_str: String = chars.iter().collect();
            let reading_str: String = mid_r[r_pos..].iter().collect();
            out.push_str(&format!("[{kanji_str}|{reading_str}]"));
            r_pos = mid_r.len();
            seg_idx += 1;
        }
    }

    if r_pos == mid_r.len() {
        Some(out)
    } else {
        None
    }
}

// ============================================================================
// Token Builders & WaniKani Enrichment
// ============================================================================

fn enrich_wanikani(surface: &str, lemma: &str, wk: &WkSubjectLookup) -> (Option<i32>, bool) {
    let kanji_chars: Vec<String> = surface
        .chars()
        .filter(|&c| is_kanji(c) && c != '々')
        .map(|c| c.to_string())
        .collect();

    let wk_level = wk
        .vocab_levels
        .get(lemma)
        .or_else(|| wk.vocab_levels.get(surface))
        .copied()
        .or_else(|| {
            kanji_chars
                .iter()
                .filter_map(|k| wk.kanji_levels.get(k).copied())
                .max()
        });

    let wk_known =
        !kanji_chars.is_empty() && kanji_chars.iter().all(|k| wk.known_kanji.contains(k));

    (wk_level, wk_known)
}

fn build_lex_token(
    surface: &str,
    lemma: &str,
    reading: &str,
    entry: &LexEntry,
    conj_label: Option<&'static str>,
    wk: &WkSubjectLookup,
) -> AnalyzedToken {
    let pos = entry.kind.pos_str();
    let mut glosses: Vec<String> = entry.glosses.iter().map(|s| (*s).to_string()).collect();
    if let Some(label) = conj_label {
        glosses.push(format!("({label})"));
    }

    let ruby = format_ruby(surface, reading);
    let (wk_level, wk_known) = enrich_wanikani(surface, lemma, wk);
    let has_kanji = surface.chars().any(is_kanji);
    let is_content_word = pos != "punctuation"
        && (!glosses.is_empty()
            || has_kanji
            || matches!(
                pos,
                "noun" | "verb" | "i-adj" | "na-adj" | "adverb" | "expression" | "pronoun"
            ));

    AnalyzedToken {
        surface: surface.to_string(),
        lemma: lemma.to_string(),
        reading: reading.to_string(),
        pos: pos.to_string(),
        glosses,
        jlpt: entry.jlpt.map(str::to_string),
        ruby,
        wk_level,
        wk_known,
        is_content_word,
    }
}

fn build_plain_token(
    surface: &str,
    reading: &str,
    pos: &str,
    is_content_word: bool,
    wk: &WkSubjectLookup,
) -> AnalyzedToken {
    let (wk_level, wk_known) = enrich_wanikani(surface, surface, wk);
    AnalyzedToken {
        surface: surface.to_string(),
        lemma: surface.to_string(),
        reading: reading.to_string(),
        pos: pos.to_string(),
        glosses: Vec::new(),
        jlpt: None,
        ruby: surface.to_string(),
        wk_level,
        wk_known,
        is_content_word,
    }
}

fn has_known_prefix(rest: &str, lex: &LexiconIndex) -> bool {
    let offsets: Vec<usize> = rest
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(rest.len()))
        .collect();
    let count = offsets.len().saturating_sub(1);
    let max_check = count.min(8);
    for len in (2..=max_check).rev() {
        let slice = &rest[..offsets[len]];
        if lex.by_surface.contains_key(slice) {
            return true;
        }
    }
    false
}

// ============================================================================
// Morphological Deconjugation Engine
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordKind {
    Noun,
    SuruNoun,
    Godan,
    Ichidan,
    SuruVerb,
    KuruVerb,
    IAdj,
    NaAdj,
    Adverb,
    Particle,
    Auxiliary,
    Conjunction,
    Expression,
    Pronoun,
    Number,
}

impl WordKind {
    fn pos_str(self) -> &'static str {
        match self {
            WordKind::Noun | WordKind::SuruNoun => "noun",
            WordKind::Godan | WordKind::Ichidan | WordKind::SuruVerb | WordKind::KuruVerb => "verb",
            WordKind::IAdj => "i-adj",
            WordKind::NaAdj => "na-adj",
            WordKind::Adverb => "adverb",
            WordKind::Particle => "particle",
            WordKind::Auxiliary => "auxiliary",
            WordKind::Conjunction => "conjunction",
            WordKind::Expression => "expression",
            WordKind::Pronoun => "pronoun",
            WordKind::Number => "number",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetClass {
    Godan,
    Ichidan,
    Suru,
    Kuru,
    IAdj,
    NaAdj,
}

impl TargetClass {
    fn matches(self, kind: WordKind) -> bool {
        match self {
            TargetClass::Godan => kind == WordKind::Godan,
            TargetClass::Ichidan => kind == WordKind::Ichidan,
            TargetClass::Suru => kind == WordKind::SuruVerb,
            TargetClass::Kuru => kind == WordKind::KuruVerb,
            TargetClass::IAdj => kind == WordKind::IAdj,
            TargetClass::NaAdj => kind == WordKind::NaAdj,
        }
    }
}

struct DeconjMatch {
    entry: &'static LexEntry,
    label: &'static str,
    derived_reading: String,
}

fn is_suru_noun(surface: &str, lex: &LexiconIndex) -> bool {
    lex.by_surface
        .get(surface)
        .is_some_and(|entries| entries.iter().any(|e| e.kind == WordKind::SuruNoun))
}

fn is_in_lexicon_with_class(lemma: &str, cls: TargetClass, lex: &LexiconIndex) -> bool {
    lex.by_surface
        .get(lemma)
        .is_some_and(|entries| entries.iter().any(|e| cls.matches(e.kind)))
}

fn find_lex_entry_with_class(
    lemma: &str,
    cls: TargetClass,
    lex: &LexiconIndex,
) -> Option<&'static LexEntry> {
    lex.by_surface
        .get(lemma)?
        .iter()
        .copied()
        .find(|e| cls.matches(e.kind))
}

/// Attempt to deconjugate a candidate substring during morphological segmentation.
fn deconjugate_for_segmenter(
    surface: &str,
    next_text: &str,
    lex: &LexiconIndex,
) -> Option<DeconjMatch> {
    // 1. Direct single-step inflections
    let step1 = generate_single_step_candidates(surface);
    for (lemma, label, cls) in &step1 {
        if let Some(entry) = find_lex_entry_with_class(lemma, *cls, lex) {
            let derived_reading = derive_inflected_reading(surface, entry.surface, entry.reading);
            return Some(DeconjMatch {
                entry,
                label,
                derived_reading,
            });
        }
    }

    // 2. Two-step voice chains (potential/passive/causative + outer inflection)
    //    e.g., 読めなかった -> 読める -> 読む, 行われた -> 行われる -> 行う
    for (mid, label1, mid_cls) in &step1 {
        if *mid_cls != TargetClass::Ichidan {
            continue;
        }
        for (lemma2, label2, cls2) in generate_secondary_voice_candidates(mid) {
            if let Some(entry) = find_lex_entry_with_class(&lemma2, cls2, lex) {
                let derived_reading =
                    derive_inflected_reading(surface, entry.surface, entry.reading);
                return Some(DeconjMatch {
                    entry,
                    label: combine_voice_label(label2, label1),
                    derived_reading,
                });
            }
        }
    }

    // 3. Bare masu-stem ONLY when containing kanji (>= 2 chars) AND followed by a stem-attaching suffix
    if surface.chars().count() >= 2
        && surface.chars().any(is_kanji)
        && is_masu_stem_follower(next_text)
    {
        for (lemma, cls) in generate_masu_stem_candidates(surface) {
            if let Some(entry) = find_lex_entry_with_class(&lemma, cls, lex) {
                let derived_reading =
                    derive_inflected_reading(surface, entry.surface, entry.reading);
                return Some(DeconjMatch {
                    entry,
                    label: "masu-stem",
                    derived_reading,
                });
            }
        }
    }

    None
}

fn is_masu_stem_follower(next_text: &str) -> bool {
    const FOLLOWERS: &[&str] = &[
        "ながら",
        "やすい",
        "にくい",
        "すぎる",
        "すぎ",
        "方",
        "そう",
        "なさい",
        "始める",
        "始め",
        "終わる",
        "終わ",
        "続ける",
        "続け",
        "合う",
        "合",
    ];
    FOLLOWERS.iter().any(|&f| next_text.starts_with(f))
}

fn generate_masu_stem_candidates(surface: &str) -> Vec<(String, TargetClass)> {
    let mut out = Vec::new();
    // Godan i-stem -> u-ending
    const I_TO_U: &[(char, char)] = &[
        ('い', 'う'),
        ('き', 'く'),
        ('ぎ', 'ぐ'),
        ('し', 'す'),
        ('ち', 'つ'),
        ('に', 'ぬ'),
        ('び', 'ぶ'),
        ('み', 'む'),
        ('り', 'る'),
    ];
    if let Some(last) = surface.chars().last() {
        let prefix = &surface[..surface.len() - last.len_utf8()];
        for &(i_ch, u_ch) in I_TO_U {
            if last == i_ch && !prefix.is_empty() {
                out.push((format!("{prefix}{u_ch}"), TargetClass::Godan));
            }
        }
    }
    // Ichidan stem + る
    out.push((format!("{surface}る"), TargetClass::Ichidan));
    out
}

/// Generate single-step verb & i-adjective deconjugation candidates.
fn generate_single_step_candidates(surface: &str) -> Vec<(String, &'static str, TargetClass)> {
    let mut out = Vec::new();

    // --- Irregular する ---
    const SURU_FORMS: &[(&str, &str)] = &[
        ("しませんでした", "polite-negative-past"),
        ("しません", "polite-negative"),
        ("しました", "polite-past"),
        ("します", "polite"),
        ("しましょう", "polite-volitional"),
        ("しなければ", "negative-conditional"),
        ("しなくて", "negative-te"),
        ("しなかった", "negative-past"),
        ("しない", "negative"),
        ("したくなかった", "desiderative-negative-past"),
        ("したかった", "desiderative-past"),
        ("したくない", "desiderative-negative"),
        ("したい", "desiderative"),
        ("したら", "conditional-tara"),
        ("して", "te-form"),
        ("した", "past"),
        ("しよう", "volitional"),
        ("すれば", "conditional"),
        ("される", "passive"),
        ("させる", "causative"),
        ("せずに", "negative-literary"),
        ("せず", "negative-literary"),
    ];
    for &(form, label) in SURU_FORMS {
        if surface == form {
            out.push(("する".to_string(), label, TargetClass::Suru));
        }
    }

    // --- Irregular 来る / くる ---
    const KURU_KANJI_FORMS: &[(&str, &str)] = &[
        ("来ませんでした", "polite-negative-past"),
        ("来ません", "polite-negative"),
        ("来ました", "polite-past"),
        ("来ます", "polite"),
        ("来ましょう", "polite-volitional"),
        ("来なければ", "negative-conditional"),
        ("来なくて", "negative-te"),
        ("来なかった", "negative-past"),
        ("来ない", "negative"),
        ("来たくなかった", "desiderative-negative-past"),
        ("来たかった", "desiderative-past"),
        ("来たくない", "desiderative-negative"),
        ("来たい", "desiderative"),
        ("来たら", "conditional-tara"),
        ("来て", "te-form"),
        ("来た", "past"),
        ("来よう", "volitional"),
        ("来れば", "conditional"),
        ("来られる", "potential/passive"),
        ("来させる", "causative"),
    ];
    for &(form, label) in KURU_KANJI_FORMS {
        if surface == form {
            out.push(("来る".to_string(), label, TargetClass::Kuru));
        }
    }

    const KURU_KANA_FORMS: &[(&str, &str)] = &[
        ("きませんでした", "polite-negative-past"),
        ("きません", "polite-negative"),
        ("きました", "polite-past"),
        ("きます", "polite"),
        ("こなければ", "negative-conditional"),
        ("こなかった", "negative-past"),
        ("こない", "negative"),
        ("こよう", "volitional"),
        ("くれば", "conditional"),
        ("こられる", "potential/passive"),
    ];
    for &(form, label) in KURU_KANA_FORMS {
        if surface == form {
            out.push(("くる".to_string(), label, TargetClass::Kuru));
        }
    }

    // --- Irregular いい / 良い ---
    const II_FORMS: &[(&str, &str, &str)] = &[
        ("よくなかった", "いい", "negative-past"),
        ("よくなければ", "いい", "negative-conditional"),
        ("よくなくて", "いい", "negative-te"),
        ("よくない", "いい", "negative"),
        ("よかったら", "いい", "conditional-tara"),
        ("よかった", "いい", "past"),
        ("よければ", "いい", "conditional"),
        ("よくて", "いい", "te-form"),
        ("良くなかった", "良い", "negative-past"),
        ("良くなければ", "良い", "negative-conditional"),
        ("良くなくて", "良い", "negative-te"),
        ("良くない", "良い", "negative"),
        ("良かったら", "良い", "conditional-tara"),
        ("良かった", "良い", "past"),
        ("良ければ", "良い", "conditional"),
        ("良くて", "良い", "te-form"),
    ];
    for &(form, lemma, label) in II_FORMS {
        if surface == form {
            out.push((lemma.to_string(), label, TargetClass::IAdj));
        }
    }

    // --- Irregular 行く te/ta/tara exception ---
    for &(form, lemma, label) in &[
        ("行って", "行く", "te-form"),
        ("行った", "行く", "past"),
        ("行ったら", "行く", "conditional-tara"),
        ("いって", "いく", "te-form"),
        ("いった", "いく", "past"),
        ("いったら", "いく", "conditional-tara"),
    ] {
        if surface == form {
            out.push((lemma.to_string(), label, TargetClass::Godan));
        }
    }

    // --- Godan verb inflections ---
    // 1) Polite & desiderative (i-stem + suffix)
    const GODAN_I_SUFFIXES: &[(&str, &str)] = &[
        ("ませんでした", "polite-negative-past"),
        ("ません", "polite-negative"),
        ("ました", "polite-past"),
        ("ます", "polite"),
        ("ましょう", "polite-volitional"),
        ("たくなかった", "desiderative-negative-past"),
        ("たかった", "desiderative-past"),
        ("たくない", "desiderative-negative"),
        ("たい", "desiderative"),
    ];
    const I_TO_U: &[(char, char)] = &[
        ('い', 'う'),
        ('き', 'く'),
        ('ぎ', 'ぐ'),
        ('し', 'す'),
        ('ち', 'つ'),
        ('に', 'ぬ'),
        ('び', 'ぶ'),
        ('み', 'む'),
        ('り', 'る'),
    ];
    for &(suffix, label) in GODAN_I_SUFFIXES {
        if let Some(stem_with_i) = surface.strip_suffix(suffix)
            && let Some(last) = stem_with_i.chars().last()
        {
            let stem = &stem_with_i[..stem_with_i.len() - last.len_utf8()];
            if !stem.is_empty() {
                for &(i_ch, u_ch) in I_TO_U {
                    if last == i_ch {
                        out.push((format!("{stem}{u_ch}"), label, TargetClass::Godan));
                    }
                }
            }
        }
    }

    // 2) Negative, passive, causative (a-stem + suffix)
    const GODAN_A_SUFFIXES: &[(&str, &str)] = &[
        ("なければ", "negative-conditional"),
        ("なくて", "negative-te"),
        ("なかった", "negative-past"),
        ("ない", "negative"),
        ("ずに", "negative-literary"),
        ("ず", "negative-literary"),
        ("れる", "passive"),
        ("せる", "causative"),
    ];
    const A_TO_U: &[(char, char)] = &[
        ('わ', 'う'),
        ('か', 'く'),
        ('が', 'ぐ'),
        ('さ', 'す'),
        ('た', 'つ'),
        ('な', 'ぬ'),
        ('ば', 'ぶ'),
        ('ま', 'む'),
        ('ら', 'る'),
    ];
    for &(suffix, label) in GODAN_A_SUFFIXES {
        if let Some(stem_with_a) = surface.strip_suffix(suffix)
            && let Some(last) = stem_with_a.chars().last()
        {
            let stem = &stem_with_a[..stem_with_a.len() - last.len_utf8()];
            if !stem.is_empty() {
                for &(a_ch, u_ch) in A_TO_U {
                    if last == a_ch {
                        out.push((format!("{stem}{u_ch}"), label, TargetClass::Godan));
                    }
                }
            }
        }
    }

    // 3) Potential (e-stem + る) & Conditional (e-stem + ば)
    const E_TO_U: &[(char, char)] = &[
        ('え', 'う'),
        ('け', 'く'),
        ('げ', 'ぐ'),
        ('せ', 'す'),
        ('て', 'つ'),
        ('ね', 'ぬ'),
        ('べ', 'ぶ'),
        ('め', 'む'),
        ('れ', 'る'),
    ];
    for &(suffix, label) in &[("る", "potential"), ("ば", "conditional")] {
        if let Some(stem_with_e) = surface.strip_suffix(suffix)
            && let Some(last) = stem_with_e.chars().last()
        {
            let stem = &stem_with_e[..stem_with_e.len() - last.len_utf8()];
            if !stem.is_empty() {
                for &(e_ch, u_ch) in E_TO_U {
                    if last == e_ch {
                        out.push((format!("{stem}{u_ch}"), label, TargetClass::Godan));
                    }
                }
            }
        }
    }

    // 4) Volitional (o-stem + う)
    const O_TO_U: &[(char, char)] = &[
        ('お', 'う'),
        ('こ', 'く'),
        ('ご', 'ぐ'),
        ('そ', 'す'),
        ('と', 'つ'),
        ('の', 'ぬ'),
        ('ぼ', 'ぶ'),
        ('も', 'む'),
        ('ろ', 'る'),
    ];
    if let Some(stem_with_o) = surface.strip_suffix('う')
        && let Some(last) = stem_with_o.chars().last()
    {
        let stem = &stem_with_o[..stem_with_o.len() - last.len_utf8()];
        if !stem.is_empty() {
            for &(o_ch, u_ch) in O_TO_U {
                if last == o_ch {
                    out.push((format!("{stem}{u_ch}"), "volitional", TargetClass::Godan));
                }
            }
        }
    }

    // 5) Te-form / Ta-form / Tara-form (Godan euphonic changes)
    const GODAN_TETA: &[(&str, &[char], &str)] = &[
        ("ったら", &['る', 'う', 'つ'], "conditional-tara"),
        ("って", &['る', 'う', 'つ'], "te-form"),
        ("った", &['る', 'う', 'つ'], "past"),
        ("いたら", &['く'], "conditional-tara"),
        ("いて", &['く'], "te-form"),
        ("いた", &['く'], "past"),
        ("いだら", &['ぐ'], "conditional-tara"),
        ("いで", &['ぐ'], "te-form"),
        ("いだ", &['ぐ'], "past"),
        ("したら", &['す'], "conditional-tara"),
        ("して", &['す'], "te-form"),
        ("した", &['す'], "past"),
        ("んだら", &['む', 'ぶ', 'ぬ'], "conditional-tara"),
        ("んで", &['む', 'ぶ', 'ぬ'], "te-form"),
        ("んだ", &['む', 'ぶ', 'ぬ'], "past"),
    ];
    for &(suffix, endings, label) in GODAN_TETA {
        if let Some(stem) = surface.strip_suffix(suffix)
            && !stem.is_empty()
        {
            for &end_ch in endings {
                out.push((format!("{stem}{end_ch}"), label, TargetClass::Godan));
            }
        }
    }

    // --- Ichidan verb inflections (stem + suffix -> stem + る) ---
    const ICHIDAN_SUFFIXES: &[(&str, &str)] = &[
        ("ませんでした", "polite-negative-past"),
        ("ません", "polite-negative"),
        ("ました", "polite-past"),
        ("ます", "polite"),
        ("ましょう", "polite-volitional"),
        ("なければ", "negative-conditional"),
        ("なくて", "negative-te"),
        ("なかった", "negative-past"),
        ("ない", "negative"),
        ("たくなかった", "desiderative-negative-past"),
        ("たかった", "desiderative-past"),
        ("たくない", "desiderative-negative"),
        ("たい", "desiderative"),
        ("られる", "potential/passive"),
        ("させる", "causative"),
        ("れる", "potential-casual"),
        ("よう", "volitional"),
        ("れば", "conditional"),
        ("たら", "conditional-tara"),
        ("ずに", "negative-literary"),
        ("ず", "negative-literary"),
        ("て", "te-form"),
        ("た", "past"),
        ("ろ", "imperative"),
    ];
    for &(suffix, label) in ICHIDAN_SUFFIXES {
        if let Some(stem) = surface.strip_suffix(suffix)
            && !stem.is_empty()
        {
            out.push((format!("{stem}る"), label, TargetClass::Ichidan));
        }
    }

    // --- い-Adjective inflections (stem + suffix -> stem + い) ---
    const IADJ_SUFFIXES: &[(&str, &str)] = &[
        ("くなかった", "negative-past"),
        ("くなければ", "negative-conditional"),
        ("くなくて", "negative-te"),
        ("くない", "negative"),
        ("かったら", "conditional-tara"),
        ("かった", "past"),
        ("ければ", "conditional"),
        ("くて", "te-form"),
        ("く", "adverbial"),
    ];
    for &(suffix, label) in IADJ_SUFFIXES {
        if let Some(stem) = surface.strip_suffix(suffix)
            && !stem.is_empty()
            // Guard against 1-char pure kana stems producing spurious い-adjs for く
            && (suffix != "く" || stem.chars().any(is_kanji))
        {
            out.push((format!("{stem}い"), label, TargetClass::IAdj));
        }
    }

    out
}

/// Secondary voice deinflection for ichidan-shaped potential/passive/causative stems.
/// Example: `"読める"` -> `"読む"`, `"行われる"` -> `"行う"`, `"食べさせる"` -> `"食べる"`.
fn generate_secondary_voice_candidates(mid: &str) -> Vec<(String, &'static str, TargetClass)> {
    let mut out = Vec::new();

    // Irregular suru/kuru voice stems
    if mid == "される" {
        out.push(("する".to_string(), "passive", TargetClass::Suru));
    } else if mid == "させる" {
        out.push(("する".to_string(), "causative", TargetClass::Suru));
    } else if mid == "来られる" {
        out.push(("来る".to_string(), "potential/passive", TargetClass::Kuru));
    } else if mid == "来させる" {
        out.push(("来る".to_string(), "causative", TargetClass::Kuru));
    }

    // Ichidan potential/passive/causative
    if let Some(stem) = mid.strip_suffix("られる")
        && !stem.is_empty()
    {
        out.push((format!("{stem}る"), "potential/passive", TargetClass::Ichidan));
    }
    if let Some(stem) = mid.strip_suffix("させる")
        && !stem.is_empty()
    {
        out.push((format!("{stem}る"), "causative", TargetClass::Ichidan));
    }

    // Godan passive (a-stem + れる) & causative (a-stem + せる)
    const A_TO_U: &[(char, char)] = &[
        ('わ', 'う'),
        ('か', 'く'),
        ('が', 'ぐ'),
        ('さ', 'す'),
        ('た', 'つ'),
        ('な', 'ぬ'),
        ('ば', 'ぶ'),
        ('ま', 'む'),
        ('ら', 'る'),
    ];
    for &(suffix, label) in &[("れる", "passive"), ("せる", "causative")] {
        if let Some(stem_a) = mid.strip_suffix(suffix)
            && let Some(last) = stem_a.chars().last()
        {
            let stem = &stem_a[..stem_a.len() - last.len_utf8()];
            if !stem.is_empty() {
                for &(a_ch, u_ch) in A_TO_U {
                    if last == a_ch {
                        out.push((format!("{stem}{u_ch}"), label, TargetClass::Godan));
                    }
                }
            }
        }
    }

    // Godan potential (e-stem + る)
    const E_TO_U: &[(char, char)] = &[
        ('え', 'う'),
        ('け', 'く'),
        ('げ', 'ぐ'),
        ('せ', 'す'),
        ('て', 'つ'),
        ('ね', 'ぬ'),
        ('べ', 'ぶ'),
        ('め', 'む'),
        ('れ', 'る'),
    ];
    if let Some(stem_e) = mid.strip_suffix('る')
        && let Some(last) = stem_e.chars().last()
    {
        let stem = &stem_e[..stem_e.len() - last.len_utf8()];
        if !stem.is_empty() {
            for &(e_ch, u_ch) in E_TO_U {
                if last == e_ch {
                    out.push((format!("{stem}{u_ch}"), "potential", TargetClass::Godan));
                }
            }
        }
    }

    out
}

fn combine_voice_label(voice: &'static str, outer: &'static str) -> &'static str {
    match (voice, outer) {
        ("potential", "negative") => "potential-negative",
        ("potential", "negative-past") => "potential-negative-past",
        ("potential", "past") => "potential-past",
        ("potential", "polite") => "potential-polite",
        ("potential", "polite-past") => "potential-polite-past",
        ("potential", "polite-negative") => "potential-polite-negative",
        ("potential", "te-form") => "potential-te",
        ("passive", "past") => "passive-past",
        ("passive", "negative") => "passive-negative",
        ("passive", "polite") => "passive-polite",
        ("passive", "polite-past") => "passive-polite-past",
        ("passive", "te-form") => "passive-te",
        ("causative", "past") => "causative-past",
        ("causative", "negative") => "causative-negative",
        ("causative", "polite") => "causative-polite",
        ("causative", "te-form") => "causative-te",
        ("potential/passive", "past") => "potential/passive-past",
        ("potential/passive", "negative") => "potential/passive-negative",
        ("potential/passive", "polite") => "potential/passive-polite",
        _ => voice,
    }
}

/// Derive the hiragana reading of an inflected surface from its dictionary lemma & reading.
fn derive_inflected_reading(surface: &str, lemma: &str, lemma_reading: &str) -> String {
    if !surface.chars().any(is_kanji) {
        return katakana_to_hiragana(surface);
    }

    // Irregular 来る kanji reading shifts: く (plain/ba), き (masu/te/ta/tai), こ (nai/you/rareru/saseru)
    if lemma == "来る"
        && let Some(tail) = surface.strip_prefix('来')
    {
        let kanji_read = if tail.starts_with("な")
            || tail.starts_with("よ")
            || tail.starts_with("ら")
            || tail.starts_with("さ")
        {
            "こ"
        } else if tail.starts_with("れ") || tail == "る" {
            "く"
        } else {
            "き"
        };
        return format!("{kanji_read}{}", katakana_to_hiragana(tail));
    }

    // Shared prefix alignment: e.g. lemma="読む" (よむ), surface="読める" -> shared="読",
    // lemma_tail="む", stem_reading="よ", surface_tail="める" => "よめる".
    let s_chars: Vec<char> = surface.chars().collect();
    let l_chars: Vec<char> = lemma.chars().collect();
    let mut common = 0usize;
    while common < s_chars.len() && common < l_chars.len() && s_chars[common] == l_chars[common] {
        common += 1;
    }

    let lemma_tail: String = l_chars[common..]
        .iter()
        .map(|&c| kana_char_to_hiragana(c))
        .collect();
    let surface_tail: String = s_chars[common..]
        .iter()
        .map(|&c| kana_char_to_hiragana(c))
        .collect();

    if let Some(stem_reading) = lemma_reading.strip_suffix(&lemma_tail) {
        return format!("{stem_reading}{surface_tail}");
    }

    lemma_reading.to_string()
}

// ============================================================================
// Embedded JMdict-Core Lexicon
// ============================================================================

#[derive(Debug, Clone, Copy)]
struct LexEntry {
    surface: &'static str,
    lemma: Option<&'static str>,
    reading: &'static str,
    kind: WordKind,
    jlpt: Option<&'static str>,
    glosses: &'static [&'static str],
}

struct LexiconIndex {
    by_surface: HashMap<&'static str, Vec<&'static LexEntry>>,
}

fn lexicon() -> &'static LexiconIndex {
    static INDEX: OnceLock<LexiconIndex> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut by_surface: HashMap<&'static str, Vec<&'static LexEntry>> =
            HashMap::with_capacity(EMBEDDED_LEXICON.len());
        for entry in EMBEDDED_LEXICON {
            by_surface.entry(entry.surface).or_default().push(entry);
        }
        LexiconIndex { by_surface }
    })
}

const fn lex(
    surface: &'static str,
    reading: &'static str,
    kind: WordKind,
    jlpt: Option<&'static str>,
    glosses: &'static [&'static str],
) -> LexEntry {
    LexEntry {
        surface,
        lemma: None,
        reading,
        kind,
        jlpt,
        glosses,
    }
}

const fn lex_lemma(
    surface: &'static str,
    lemma: &'static str,
    reading: &'static str,
    kind: WordKind,
    jlpt: Option<&'static str>,
    glosses: &'static [&'static str],
) -> LexEntry {
    LexEntry {
        surface,
        lemma: Some(lemma),
        reading,
        kind,
        jlpt,
        glosses,
    }
}

static EMBEDDED_LEXICON: &[LexEntry] = &[
    // ==================== Grammar Expressions & Auxiliaries ====================
    lex("にもかかわらず", "にもかかわらず", WordKind::Expression, Some("N2"), &["in spite of", "despite (formal)"]),
    lex("にも関わらず", "にもかかわらず", WordKind::Expression, Some("N2"), &["in spite of", "despite (formal)"]),
    lex("わけにはいかない", "わけにはいかない", WordKind::Expression, Some("N2"), &["cannot possibly", "cannot bring oneself to"]),
    lex("わけではない", "わけではない", WordKind::Expression, Some("N3"), &["it does not mean that", "not necessarily"]),
    lex("ざるを得ない", "ざるをえない", WordKind::Expression, Some("N1"), &["have no choice but to", "cannot help but"]),
    lex("ざるをえない", "ざるをえない", WordKind::Expression, Some("N1"), &["have no choice but to", "cannot help but"]),
    lex("に違いない", "にちがいない", WordKind::Expression, Some("N2"), &["must be", "no doubt that"]),
    lex("にちがいない", "にちがいない", WordKind::Expression, Some("N2"), &["must be", "no doubt that"]),
    lex("おかげで", "おかげで", WordKind::Expression, Some("N3"), &["thanks to", "owing to (positive outcome)"]),
    lex("お陰で", "おかげで", WordKind::Expression, Some("N3"), &["thanks to", "owing to (positive outcome)"]),
    lex("せいで", "せいで", WordKind::Expression, Some("N3"), &["because of", "fault of (negative outcome)"]),
    lex("によって", "によって", WordKind::Expression, Some("N3"), &["depending on", "by means of", "due to"]),
    lex("によっては", "によっては", WordKind::Expression, Some("N3"), &["depending on", "in some cases"]),
    lex("による", "による", WordKind::Expression, Some("N3"), &["due to", "by means of", "according to"]),
    lex("により", "により", WordKind::Expression, Some("N2"), &["due to", "by means of (formal)"]),
    lex("について", "について", WordKind::Expression, Some("N4"), &["about", "concerning", "regarding"]),
    lex("にとって", "にとって", WordKind::Expression, Some("N3"), &["for", "to", "from the standpoint of"]),
    lex("に対して", "にたいして", WordKind::Expression, Some("N3"), &["toward", "against", "in contrast to"]),
    lex("において", "において", WordKind::Expression, Some("N2"), &["in", "at", "regarding (formal)"]),
    lex("に関して", "にかんして", WordKind::Expression, Some("N2"), &["regarding", "concerning"]),
    lex("を通じて", "をつうじて", WordKind::Expression, Some("N2"), &["through", "throughout"]),
    lex("かわりに", "かわりに", WordKind::Expression, Some("N3"), &["instead of", "in exchange for"]),
    lex("代わりに", "かわりに", WordKind::Expression, Some("N3"), &["instead of", "in exchange for"]),
    lex("うちに", "うちに", WordKind::Expression, Some("N3"), &["while (before conditions change)"]),
    lex("内に", "うちに", WordKind::Expression, Some("N3"), &["while", "within"]),
    lex("ために", "ために", WordKind::Expression, Some("N4"), &["in order to", "for the sake of"]),
    lex("ための", "ための", WordKind::Expression, Some("N4"), &["for the purpose of", "intended for"]),
    lex("ように", "ように", WordKind::Expression, Some("N4"), &["so that", "in order to", "like / as"]),
    lex("つもり", "つもり", WordKind::Noun, Some("N4"), &["intention", "plan"]),
    lex("はず", "はず", WordKind::Noun, Some("N4"), &["expectation", "should be (logical deduction)"]),
    lex("わけ", "わけ", WordKind::Noun, Some("N3"), &["reason", "meaning", "circumstance"]),
    lex("一方", "いっぽう", WordKind::Noun, Some("N2"), &["one way", "on the other hand", "continuing trend"]),
    lex("ほう", "ほう", WordKind::Noun, Some("N5"), &["direction", "side", "alternative"]),
    lex("方", "ほう", WordKind::Noun, Some("N5"), &["direction", "side", "way of"]),
    lex("べき", "べき", WordKind::Auxiliary, Some("N3"), &["should", "ought to"]),
    lex("かもしれない", "かもしれない", WordKind::Expression, Some("N4"), &["might", "may", "perhaps"]),
    lex("かもしれません", "かもしれません", WordKind::Expression, Some("N4"), &["might", "may (polite)"]),
    lex("らしい", "らしい", WordKind::Auxiliary, Some("N4"), &["seems like", "apparently", "typical of"]),
    lex("みたい", "みたい", WordKind::Auxiliary, Some("N4"), &["like", "similar to", "seems"]),
    lex("まま", "まま", WordKind::Noun, Some("N4"), &["as it is", "remaining in a state"]),
    lex("ところ", "ところ", WordKind::Noun, Some("N4"), &["place", "point in time", "about to / just did"]),
    lex("とおり", "とおり", WordKind::Noun, Some("N4"), &["in the same way as", "as expected"]),
    lex("通り", "とおり", WordKind::Noun, Some("N4"), &["street", "way", "as (expected/scheduled)"]),
    lex("ください", "ください", WordKind::Auxiliary, Some("N5"), &["please (do/give)"]),
    lex("下さい", "ください", WordKind::Auxiliary, Some("N5"), &["please (do/give)"]),
    lex_lemma("ではありません", "ではない", "ではありません", WordKind::Auxiliary, Some("N5"), &["is not (formal polite negative copula)"]),
    lex_lemma("じゃありません", "じゃない", "じゃありません", WordKind::Auxiliary, Some("N5"), &["is not (polite negative copula)"]),
    lex("ではない", "ではない", WordKind::Auxiliary, Some("N4"), &["is not (formal negative copula)"]),
    lex("じゃない", "じゃない", WordKind::Auxiliary, Some("N5"), &["is not (casual negative copula)"]),
    lex_lemma("でした", "です", "でした", WordKind::Auxiliary, Some("N5"), &["was (polite past copula)"]),
    lex("です", "です", WordKind::Auxiliary, Some("N5"), &["be", "is (polite copula)"]),
    lex_lemma("だった", "だ", "だった", WordKind::Auxiliary, Some("N5"), &["was (plain past copula)"]),
    lex("だ", "だ", WordKind::Auxiliary, Some("N5"), &["be", "is (plain copula)"]),
    lex("でしょう", "でしょう", WordKind::Auxiliary, Some("N5"), &["probably", "right? (polite)"]),
    lex("だろう", "だろう", WordKind::Auxiliary, Some("N4"), &["probably", "I suppose (plain)"]),

    // ==================== Particles & Conjunctions ====================
    lex("は", "は", WordKind::Particle, Some("N5"), &["topic marker"]),
    lex("が", "が", WordKind::Particle, Some("N5"), &["subject marker", "but (conjunctive)"]),
    lex("を", "を", WordKind::Particle, Some("N5"), &["direct object marker"]),
    lex("に", "に", WordKind::Particle, Some("N5"), &["in", "at", "to", "indirect object marker"]),
    lex("で", "で", WordKind::Particle, Some("N5"), &["at (location of action)", "by means of"]),
    lex("へ", "へ", WordKind::Particle, Some("N5"), &["toward", "to (direction)"]),
    lex("と", "と", WordKind::Particle, Some("N5"), &["and", "with", "quotation marker"]),
    lex("も", "も", WordKind::Particle, Some("N5"), &["also", "too", "even"]),
    lex("の", "の", WordKind::Particle, Some("N5"), &["possessive", "of", "nominalizer"]),
    lex("や", "や", WordKind::Particle, Some("N5"), &["and (non-exhaustive list)"]),
    lex("か", "か", WordKind::Particle, Some("N5"), &["question marker", "or"]),
    lex("ね", "ね", WordKind::Particle, Some("N5"), &["isn't it?", "right? (seeking agreement)"]),
    lex("よ", "よ", WordKind::Particle, Some("N5"), &["emphasis / informing listener"]),
    lex("な", "な", WordKind::Particle, Some("N5"), &["na-adjective connector", "don't (prohibitive)"]),
    lex("わ", "わ", WordKind::Particle, Some("N4"), &["emphatic sentence-ending particle"]),
    lex("から", "から", WordKind::Particle, Some("N5"), &["from", "because", "since"]),
    lex("まで", "まで", WordKind::Particle, Some("N5"), &["until", "up to", "as far as"]),
    lex("より", "より", WordKind::Particle, Some("N5"), &["than", "more than", "from"]),
    lex("だけ", "だけ", WordKind::Particle, Some("N5"), &["only", "just"]),
    lex("しか", "しか", WordKind::Particle, Some("N4"), &["only (used with negative)"]),
    lex("ばかり", "ばかり", WordKind::Particle, Some("N3"), &["nothing but", "just finished doing"]),
    lex("ばっかり", "ばっかり", WordKind::Particle, Some("N3"), &["nothing but (casual)"]),
    lex("ほど", "ほど", WordKind::Particle, Some("N4"), &["to the extent of", "about", "as much as"]),
    lex("くらい", "くらい", WordKind::Particle, Some("N5"), &["approximately", "about"]),
    lex("ぐらい", "ぐらい", WordKind::Particle, Some("N5"), &["approximately", "about"]),
    lex("など", "など", WordKind::Particle, Some("N4"), &["and so on", "etc."]),
    lex("ながら", "ながら", WordKind::Particle, Some("N4"), &["while doing (simultaneous action)"]),
    lex("のに", "のに", WordKind::Conjunction, Some("N4"), &["even though", "despite (unexpected)"]),
    lex("ので", "ので", WordKind::Conjunction, Some("N4"), &["because", "since (objective/soft)"]),
    lex("けど", "けど", WordKind::Conjunction, Some("N5"), &["but", "however", "although"]),
    lex("けれど", "けれど", WordKind::Conjunction, Some("N4"), &["but", "however"]),
    lex("けれども", "けれども", WordKind::Conjunction, Some("N4"), &["but", "however (polite)"]),
    lex("ても", "ても", WordKind::Particle, Some("N5"), &["even if", "even though"]),
    lex("でも", "でも", WordKind::Conjunction, Some("N5"), &["but", "however", "even"]),
    lex("には", "には", WordKind::Particle, Some("N5"), &["in", "to", "for (topic/contrast)"]),
    lex("では", "では", WordKind::Particle, Some("N5"), &["in", "at (topic)", "well then"]),
    lex("とは", "とは", WordKind::Particle, Some("N3"), &["as for", "what is called"]),
    lex("にも", "にも", WordKind::Particle, Some("N5"), &["also in", "even to"]),
    lex("なら", "なら", WordKind::Conjunction, Some("N4"), &["if", "in the case of"]),
    lex("しかし", "しかし", WordKind::Conjunction, Some("N4"), &["however", "but"]),
    lex("だが", "だが", WordKind::Conjunction, Some("N3"), &["however", "but (written)"]),
    lex("ところが", "ところが", WordKind::Conjunction, Some("N3"), &["however", "even so"]),
    lex("それでも", "それでも", WordKind::Conjunction, Some("N4"), &["even so", "nevertheless"]),
    lex("そして", "そして", WordKind::Conjunction, Some("N5"), &["and then", "and"]),
    lex("それから", "それから", WordKind::Conjunction, Some("N5"), &["after that", "and then"]),
    lex("それで", "それで", WordKind::Conjunction, Some("N4"), &["so", "therefore"]),
    lex("だから", "だから", WordKind::Conjunction, Some("N5"), &["so", "therefore"]),
    lex("ですから", "ですから", WordKind::Conjunction, Some("N5"), &["therefore (polite)"]),
    lex("または", "または", WordKind::Conjunction, Some("N3"), &["or", "otherwise"]),
    lex("あるいは", "あるいは", WordKind::Conjunction, Some("N2"), &["or", "possibly"]),
    lex("つまり", "つまり", WordKind::Conjunction, Some("N3"), &["in other words", "that is to say"]),
    lex("ただし", "ただし", WordKind::Conjunction, Some("N3"), &["however", "provided that"]),
    lex("しかも", "しかも", WordKind::Conjunction, Some("N3"), &["moreover", "furthermore"]),
    lex("さらに", "さらに", WordKind::Adverb, Some("N3"), &["furthermore", "even more"]),
    lex("したがって", "したがって", WordKind::Conjunction, Some("N2"), &["therefore", "consequently"]),

    // ==================== Pronouns & Demonstratives ====================
    lex("私", "わたし", WordKind::Pronoun, Some("N5"), &["I", "me"]),
    lex("僕", "ぼく", WordKind::Pronoun, Some("N4"), &["I", "me (masculine)"]),
    lex("俺", "おれ", WordKind::Pronoun, Some("N3"), &["I", "me (casual masculine)"]),
    lex("あなた", "あなた", WordKind::Pronoun, Some("N5"), &["you"]),
    lex("彼", "かれ", WordKind::Pronoun, Some("N4"), &["he", "him", "boyfriend"]),
    lex("彼女", "かのじょ", WordKind::Pronoun, Some("N4"), &["she", "her", "girlfriend"]),
    lex("私たち", "わたしたち", WordKind::Pronoun, Some("N5"), &["we", "us"]),
    lex("みんな", "みんな", WordKind::Pronoun, Some("N5"), &["everyone", "all"]),
    lex("皆様", "みなさま", WordKind::Pronoun, Some("N3"), &["everyone (honorific)"]),
    lex("誰", "だれ", WordKind::Pronoun, Some("N5"), &["who"]),
    lex("何", "なに", WordKind::Pronoun, Some("N5"), &["what"]),
    lex("これ", "これ", WordKind::Pronoun, Some("N5"), &["this (near speaker)"]),
    lex("それ", "それ", WordKind::Pronoun, Some("N5"), &["that (near listener)"]),
    lex("あれ", "あれ", WordKind::Pronoun, Some("N5"), &["that over there"]),
    lex("どれ", "どれ", WordKind::Pronoun, Some("N5"), &["which one"]),
    lex("この", "この", WordKind::Pronoun, Some("N5"), &["this (~)"]),
    lex("その", "その", WordKind::Pronoun, Some("N5"), &["that (~)"]),
    lex("あの", "あの", WordKind::Pronoun, Some("N5"), &["that (~ over there)"]),
    lex("どの", "どの", WordKind::Pronoun, Some("N5"), &["which (~)"]),
    lex("ここ", "ここ", WordKind::Pronoun, Some("N5"), &["here"]),
    lex("そこ", "そこ", WordKind::Pronoun, Some("N5"), &["there"]),
    lex("あそこ", "あそこ", WordKind::Pronoun, Some("N5"), &["over there"]),
    lex("どこ", "どこ", WordKind::Pronoun, Some("N5"), &["where"]),
    lex("こちら", "こちら", WordKind::Pronoun, Some("N5"), &["this way", "here (polite)"]),
    lex("そちら", "そちら", WordKind::Pronoun, Some("N5"), &["that way", "there (polite)"]),
    lex("そんな", "そんな", WordKind::Pronoun, Some("N4"), &["such", "that sort of"]),
    lex("こんな", "こんな", WordKind::Pronoun, Some("N4"), &["like this", "this sort of"]),

    // ==================== Greetings & Common Expressions ====================
    lex("おはよう", "おはよう", WordKind::Expression, Some("N5"), &["good morning"]),
    lex("こんにちは", "こんにちは", WordKind::Expression, Some("N5"), &["hello", "good afternoon"]),
    lex("こんばんは", "こんばんは", WordKind::Expression, Some("N5"), &["good evening"]),
    lex("ありがとう", "ありがとう", WordKind::Expression, Some("N5"), &["thank you"]),
    lex("すみません", "すみません", WordKind::Expression, Some("N5"), &["excuse me", "sorry"]),
    lex("ごめんなさい", "ごめんなさい", WordKind::Expression, Some("N5"), &["I'm sorry"]),
    lex("お願いします", "おねがいします", WordKind::Expression, Some("N5"), &["please"]),
    lex("よろしく", "よろしく", WordKind::Expression, Some("N5"), &["best regards", "please treat me well"]),

    // ==================== Time & Calendar Nouns ====================
    lex("毎日", "まいにち", WordKind::Noun, Some("N5"), &["every day", "daily"]),
    lex("毎週", "まいしゅう", WordKind::Noun, Some("N5"), &["every week"]),
    lex("毎月", "まいつき", WordKind::Noun, Some("N5"), &["every month"]),
    lex("毎年", "まいとし", WordKind::Noun, Some("N5"), &["every year"]),
    lex("毎朝", "まいあさ", WordKind::Noun, Some("N5"), &["every morning"]),
    lex("毎晩", "まいばん", WordKind::Noun, Some("N5"), &["every evening"]),
    lex("今日", "きょう", WordKind::Noun, Some("N5"), &["today"]),
    lex("昨日", "きのう", WordKind::Noun, Some("N5"), &["yesterday"]),
    lex("明日", "あした", WordKind::Noun, Some("N5"), &["tomorrow"]),
    lex("今", "いま", WordKind::Noun, Some("N5"), &["now"]),
    lex("今朝", "けさ", WordKind::Noun, Some("N5"), &["this morning"]),
    lex("今夜", "こんや", WordKind::Noun, Some("N4"), &["tonight"]),
    lex("今年", "ことし", WordKind::Noun, Some("N5"), &["this year"]),
    lex("去年", "きょねん", WordKind::Noun, Some("N5"), &["last year"]),
    lex("来年", "らいねん", WordKind::Noun, Some("N5"), &["next year"]),
    lex("今週", "こんしゅう", WordKind::Noun, Some("N5"), &["this week"]),
    lex("先週", "せんしゅう", WordKind::Noun, Some("N5"), &["last week"]),
    lex("来週", "らいしゅう", WordKind::Noun, Some("N5"), &["next week"]),
    lex("今月", "こんげつ", WordKind::Noun, Some("N5"), &["this month"]),
    lex("先月", "せんげつ", WordKind::Noun, Some("N5"), &["last month"]),
    lex("来月", "らいげつ", WordKind::Noun, Some("N5"), &["next month"]),
    lex("朝", "あさ", WordKind::Noun, Some("N5"), &["morning"]),
    lex("昼", "ひる", WordKind::Noun, Some("N5"), &["noon", "daytime"]),
    lex("夕方", "ゆうがた", WordKind::Noun, Some("N5"), &["evening", "dusk"]),
    lex("夜", "よる", WordKind::Noun, Some("N5"), &["night", "evening"]),
    lex("週末", "しゅうまつ", WordKind::Noun, Some("N5"), &["weekend"]),
    lex("平日", "へいじつ", WordKind::Noun, Some("N3"), &["weekday"]),
    lex("春", "はる", WordKind::Noun, Some("N5"), &["spring"]),
    lex("夏", "なつ", WordKind::Noun, Some("N5"), &["summer"]),
    lex("秋", "あき", WordKind::Noun, Some("N5"), &["autumn", "fall"]),
    lex("冬", "ふゆ", WordKind::Noun, Some("N5"), &["winter"]),
    lex("月曜日", "げつようび", WordKind::Noun, Some("N5"), &["Monday"]),
    lex("火曜日", "かようび", WordKind::Noun, Some("N5"), &["Tuesday"]),
    lex("水曜日", "すいようび", WordKind::Noun, Some("N5"), &["Wednesday"]),
    lex("木曜日", "もくようび", WordKind::Noun, Some("N5"), &["Thursday"]),
    lex("金曜日", "きんようび", WordKind::Noun, Some("N5"), &["Friday"]),
    lex("土曜日", "どようび", WordKind::Noun, Some("N5"), &["Saturday"]),
    lex("日曜日", "にちようび", WordKind::Noun, Some("N5"), &["Sunday"]),
    lex("時間", "じかん", WordKind::Noun, Some("N5"), &["time", "hour"]),
    lex("時", "とき", WordKind::Noun, Some("N5"), &["time", "when"]),
    lex("とき", "とき", WordKind::Noun, Some("N5"), &["time", "when"]),
    lex("分", "ふん", WordKind::Noun, Some("N5"), &["minute", "part"]),
    lex("秒", "びょう", WordKind::Noun, Some("N4"), &["second (unit of time)"]),
    lex("年", "とし", WordKind::Noun, Some("N5"), &["year", "age"]),
    lex("最近", "さいきん", WordKind::Noun, Some("N4"), &["recently", "lately"]),
    lex("将来", "しょうらい", WordKind::Noun, Some("N4"), &["future", "prospects"]),
    lex("昔", "むかし", WordKind::Noun, Some("N4"), &["old days", "long ago"]),
    lex("最初", "さいしょ", WordKind::Noun, Some("N4"), &["beginning", "first"]),
    lex("最後", "さいご", WordKind::Noun, Some("N4"), &["end", "last"]),
    lex("前", "まえ", WordKind::Noun, Some("N5"), &["before", "in front"]),
    lex("後", "あと", WordKind::Noun, Some("N5"), &["after", "later", "behind"]),
    lex("間", "あいだ", WordKind::Noun, Some("N4"), &["interval", "between", "while"]),
    lex("途中", "とちゅう", WordKind::Noun, Some("N4"), &["on the way", "midway"]),
    lex("ころ", "ころ", WordKind::Noun, Some("N4"), &["approximate time", "around when"]),

    // ==================== People & Family ====================
    lex("人々", "ひとびと", WordKind::Noun, Some("N4"), &["people", "everybody"]),
    lex("人", "ひと", WordKind::Noun, Some("N5"), &["person", "human"]),
    lex("大人", "おとな", WordKind::Noun, Some("N5"), &["adult"]),
    lex("子供", "こども", WordKind::Noun, Some("N5"), &["child", "children"]),
    lex("子ども", "こども", WordKind::Noun, Some("N5"), &["child", "children"]),
    lex("男", "おとこ", WordKind::Noun, Some("N5"), &["man", "male"]),
    lex("女", "おんな", WordKind::Noun, Some("N5"), &["woman", "female"]),
    lex("家族", "かぞく", WordKind::Noun, Some("N5"), &["family"]),
    lex("お父さん", "おとうさん", WordKind::Noun, Some("N5"), &["father", "dad"]),
    lex("父", "ちち", WordKind::Noun, Some("N5"), &["my father"]),
    lex("お母さん", "おかあさん", WordKind::Noun, Some("N5"), &["mother", "mom"]),
    lex("母", "はは", WordKind::Noun, Some("N5"), &["my mother"]),
    lex("お兄さん", "おにいさん", WordKind::Noun, Some("N5"), &["older brother"]),
    lex("兄", "あに", WordKind::Noun, Some("N5"), &["my older brother"]),
    lex("お姉さん", "おねえさん", WordKind::Noun, Some("N5"), &["older sister"]),
    lex("姉", "あね", WordKind::Noun, Some("N5"), &["my older sister"]),
    lex("弟", "おとうと", WordKind::Noun, Some("N5"), &["younger brother"]),
    lex("妹", "いもうと", WordKind::Noun, Some("N5"), &["younger sister"]),
    lex("友達", "ともだち", WordKind::Noun, Some("N5"), &["friend", "companion"]),
    lex("友人", "ゆうじん", WordKind::Noun, Some("N3"), &["friend (formal)"]),
    lex("先生", "せんせい", WordKind::Noun, Some("N5"), &["teacher", "instructor", "master"]),
    lex("教師", "きょうし", WordKind::Noun, Some("N3"), &["teacher", "educator"]),
    lex("学生", "がくせい", WordKind::Noun, Some("N5"), &["student"]),
    lex("生徒", "せいと", WordKind::Noun, Some("N4"), &["pupil", "school student"]),
    lex("医者", "いしゃ", WordKind::Noun, Some("N5"), &["doctor", "physician"]),
    lex("会社員", "かいしゃいん", WordKind::Noun, Some("N5"), &["company employee"]),
    lex("店員", "てんいん", WordKind::Noun, Some("N4"), &["shop clerk", "salesperson"]),
    lex("お客様", "おきゃくさま", WordKind::Noun, Some("N4"), &["customer", "guest (honorific)"]),
    lex("客", "きゃく", WordKind::Noun, Some("N4"), &["guest", "customer"]),
    lex("自分", "じぶん", WordKind::Noun, Some("N4"), &["oneself", "myself"]),
    lex("相手", "あいて", WordKind::Noun, Some("N3"), &["partner", "opponent", "other party"]),

    // ==================== Places, Geography & Nature ====================
    lex("日本語", "にほんご", WordKind::Noun, Some("N5"), &["Japanese language"]),
    lex("日本", "にほん", WordKind::Noun, Some("N5"), &["Japan"]),
    lex("東京", "とうきょう", WordKind::Noun, Some("N5"), &["Tokyo"]),
    lex("京都", "きょうと", WordKind::Noun, Some("N5"), &["Kyoto"]),
    lex("大阪", "おおさか", WordKind::Noun, Some("N5"), &["Osaka"]),
    lex("北海道", "ほっかいどう", WordKind::Noun, Some("N4"), &["Hokkaido"]),
    lex("富士山", "ふじさん", WordKind::Noun, Some("N5"), &["Mt. Fuji"]),
    lex("国", "くに", WordKind::Noun, Some("N5"), &["country", "nation"]),
    lex("世界", "せかい", WordKind::Noun, Some("N4"), &["world"]),
    lex("街", "まち", WordKind::Noun, Some("N4"), &["town", "city street"]),
    lex("町", "まち", WordKind::Noun, Some("N5"), &["town", "block"]),
    lex("村", "むら", WordKind::Noun, Some("N5"), &["village"]),
    lex("駅", "えき", WordKind::Noun, Some("N5"), &["train station"]),
    lex("空港", "くうこう", WordKind::Noun, Some("N4"), &["airport"]),
    lex("公園", "こうえん", WordKind::Noun, Some("N5"), &["park (public)"]),
    lex("学校", "がっこう", WordKind::Noun, Some("N5"), &["school"]),
    lex("大学", "だいがく", WordKind::Noun, Some("N5"), &["university", "college"]),
    lex("教室", "きょうしつ", WordKind::Noun, Some("N5"), &["classroom"]),
    lex("図書館", "としょかん", WordKind::Noun, Some("N5"), &["library"]),
    lex("病院", "びょういん", WordKind::Noun, Some("N5"), &["hospital"]),
    lex("銀行", "ぎんこう", WordKind::Noun, Some("N5"), &["bank"]),
    lex("郵便局", "ゆうびんきょく", WordKind::Noun, Some("N5"), &["post office"]),
    lex("会社", "かいしゃ", WordKind::Noun, Some("N5"), &["company", "corporation"]),
    lex("店", "みせ", WordKind::Noun, Some("N5"), &["shop", "store"]),
    lex("レストラン", "れすとらん", WordKind::Noun, Some("N5"), &["restaurant"]),
    lex("ホテル", "ほてる", WordKind::Noun, Some("N5"), &["hotel"]),
    lex("家", "いえ", WordKind::Noun, Some("N5"), &["house", "home"]),
    lex("部屋", "へや", WordKind::Noun, Some("N5"), &["room"]),
    lex("台所", "だいどころ", WordKind::Noun, Some("N5"), &["kitchen"]),
    lex("庭", "にわ", WordKind::Noun, Some("N5"), &["garden", "yard"]),
    lex("道路", "どうろ", WordKind::Noun, Some("N3"), &["road", "highway"]),
    lex("道", "みち", WordKind::Noun, Some("N5"), &["road", "path", "way"]),
    lex("橋", "はし", WordKind::Noun, Some("N5"), &["bridge"]),
    lex("山", "やま", WordKind::Noun, Some("N5"), &["mountain"]),
    lex("川", "かわ", WordKind::Noun, Some("N5"), &["river"]),
    lex("海", "うみ", WordKind::Noun, Some("N5"), &["sea", "ocean"]),
    lex("空", "そら", WordKind::Noun, Some("N5"), &["sky"]),
    lex("森", "もり", WordKind::Noun, Some("N4"), &["forest"]),
    lex("木", "き", WordKind::Noun, Some("N5"), &["tree", "wood"]),
    lex("花", "はな", WordKind::Noun, Some("N5"), &["flower"]),
    lex("草", "くさ", WordKind::Noun, Some("N4"), &["grass"]),
    lex("大雨", "おおあめ", WordKind::Noun, Some("N3"), &["heavy rain", "downpour"]),
    lex("雨", "あめ", WordKind::Noun, Some("N5"), &["rain"]),
    lex("雪", "ゆき", WordKind::Noun, Some("N5"), &["snow"]),
    lex("風", "かぜ", WordKind::Noun, Some("N5"), &["wind", "breeze"]),
    lex("台風", "たいふう", WordKind::Noun, Some("N4"), &["typhoon"]),
    lex("雲", "くも", WordKind::Noun, Some("N4"), &["cloud"]),
    lex("太陽", "たいよう", WordKind::Noun, Some("N3"), &["sun"]),
    lex("月", "つき", WordKind::Noun, Some("N5"), &["moon", "month"]),
    lex("星", "ほし", WordKind::Noun, Some("N4"), &["star"]),
    lex("天気", "てんき", WordKind::Noun, Some("N5"), &["weather"]),
    lex("季節", "きせつ", WordKind::Noun, Some("N4"), &["season"]),
    lex("自然", "しぜん", WordKind::NaAdj, Some("N3"), &["nature", "natural"]),
    lex("環境", "かんきょう", WordKind::Noun, Some("N3"), &["environment"]),
    lex("場所", "ばしょ", WordKind::Noun, Some("N4"), &["place", "location"]),
    lex("中", "なか", WordKind::Noun, Some("N5"), &["inside", "middle", "during"]),
    lex("外", "そと", WordKind::Noun, Some("N5"), &["outside"]),
    lex("上", "うえ", WordKind::Noun, Some("N5"), &["above", "on top"]),
    lex("下", "した", WordKind::Noun, Some("N5"), &["below", "under"]),
    lex("右", "みぎ", WordKind::Noun, Some("N5"), &["right"]),
    lex("左", "ひだり", WordKind::Noun, Some("N5"), &["left"]),
    lex("横", "よこ", WordKind::Noun, Some("N5"), &["beside", "horizontal"]),
    lex("隣", "となり", WordKind::Noun, Some("N5"), &["next to", "neighbor"]),
    lex("近く", "ちかく", WordKind::Noun, Some("N5"), &["nearby", "neighborhood"]),

    // ==================== Everyday, Study, Food, Work & Society Nouns ====================
    lex("英語", "えいご", WordKind::Noun, Some("N5"), &["English language"]),
    lex("言葉", "ことば", WordKind::Noun, Some("N5"), &["word", "language", "expression"]),
    lex("ことば", "ことば", WordKind::Noun, Some("N5"), &["word", "language"]),
    lex("漢字", "かんじ", WordKind::Noun, Some("N5"), &["kanji", "Chinese characters"]),
    lex("ひらがな", "ひらがな", WordKind::Noun, Some("N5"), &["hiragana"]),
    lex("カタカナ", "かたかな", WordKind::Noun, Some("N5"), &["katakana"]),
    lex("文字", "もじ", WordKind::Noun, Some("N4"), &["letter", "character"]),
    lex("文章", "ぶんしょう", WordKind::Noun, Some("N3"), &["sentence", "text", "essay"]),
    lex("意味", "いみ", WordKind::SuruNoun, Some("N5"), &["meaning", "significance"]),
    lex("新聞", "しんぶん", WordKind::Noun, Some("N5"), &["newspaper"]),
    lex("雑誌", "ざっし", WordKind::Noun, Some("N5"), &["magazine"]),
    lex("本", "ほん", WordKind::Noun, Some("N5"), &["book"]),
    lex("手紙", "てがみ", WordKind::Noun, Some("N5"), &["letter"]),
    lex("日記", "にっき", WordKind::Noun, Some("N4"), &["diary", "journal"]),
    lex("辞書", "じしょ", WordKind::Noun, Some("N5"), &["dictionary"]),
    lex("ノート", "のーと", WordKind::Noun, Some("N5"), &["notebook"]),
    lex("紙", "かみ", WordKind::Noun, Some("N5"), &["paper"]),
    lex("写真", "しゃしん", WordKind::Noun, Some("N5"), &["photograph", "photo"]),
    lex("映画", "えいが", WordKind::Noun, Some("N5"), &["movie", "film"]),
    lex("音楽", "おんがく", WordKind::Noun, Some("N5"), &["music"]),
    lex("歌", "うた", WordKind::Noun, Some("N5"), &["song"]),
    lex("絵", "え", WordKind::Noun, Some("N5"), &["picture", "painting"]),
    lex("テレビ", "てれび", WordKind::Noun, Some("N5"), &["television", "TV"]),
    lex("パソコン", "ぱそこん", WordKind::Noun, Some("N4"), &["personal computer"]),
    lex("スマホ", "すまほ", WordKind::Noun, Some("N4"), &["smartphone"]),
    lex("電話", "でんわ", WordKind::SuruNoun, Some("N5"), &["telephone", "phone call"]),
    lex("ゲーム", "げーむ", WordKind::Noun, Some("N5"), &["game"]),
    lex("ニュース", "にゅーす", WordKind::Noun, Some("N4"), &["news"]),
    lex("レポート", "れぽーと", WordKind::Noun, Some("N4"), &["report", "paper"]),
    lex("ピクニック", "ぴくにっく", WordKind::Noun, Some("N4"), &["picnic"]),
    lex("電車", "でんしゃ", WordKind::Noun, Some("N5"), &["train", "electric train"]),
    lex("地下鉄", "ちかてつ", WordKind::Noun, Some("N5"), &["subway", "metro"]),
    lex("バス", "ばす", WordKind::Noun, Some("N5"), &["bus"]),
    lex("タクシー", "たくしー", WordKind::Noun, Some("N5"), &["taxi"]),
    lex("車", "くるま", WordKind::Noun, Some("N5"), &["car", "vehicle"]),
    lex("自転車", "じてんしゃ", WordKind::Noun, Some("N5"), &["bicycle"]),
    lex("飛行機", "ひこうき", WordKind::Noun, Some("N5"), &["airplane"]),
    lex("船", "ふね", WordKind::Noun, Some("N4"), &["ship", "boat"]),
    lex("切符", "きっぷ", WordKind::Noun, Some("N5"), &["ticket"]),
    lex("乗り換え", "のりかえ", WordKind::SuruNoun, Some("N4"), &["transfer (trains/buses)"]),
    lex("交通", "こうつう", WordKind::Noun, Some("N4"), &["traffic", "transportation"]),
    lex("事故", "じこ", WordKind::Noun, Some("N4"), &["accident"]),
    lex("傘", "かさ", WordKind::Noun, Some("N5"), &["umbrella"]),
    lex("鞄", "かばん", WordKind::Noun, Some("N5"), &["bag", "satchel"]),
    lex("財布", "さいふ", WordKind::Noun, Some("N5"), &["wallet"]),
    lex("お金", "おかね", WordKind::Noun, Some("N5"), &["money"]),
    lex("鍵", "かぎ", WordKind::Noun, Some("N5"), &["key", "lock"]),
    lex("時計", "とけい", WordKind::Noun, Some("N5"), &["clock", "watch"]),
    lex("眼鏡", "めがね", WordKind::Noun, Some("N5"), &["glasses", "eyeglasses"]),
    lex("窓", "まど", WordKind::Noun, Some("N5"), &["window"]),
    lex("服", "ふく", WordKind::Noun, Some("N5"), &["clothes"]),
    lex("靴", "くつ", WordKind::Noun, Some("N5"), &["shoes"]),
    lex("コート", "こーと", WordKind::Noun, Some("N5"), &["coat"]),
    lex("食べ物", "たべもの", WordKind::Noun, Some("N5"), &["food"]),
    lex("飲み物", "のみもの", WordKind::Noun, Some("N5"), &["drink", "beverage"]),
    lex("朝ごはん", "あさごはん", WordKind::Noun, Some("N5"), &["breakfast"]),
    lex("昼ごはん", "ひるごはん", WordKind::Noun, Some("N5"), &["lunch"]),
    lex("晩ご飯", "ばんごはん", WordKind::Noun, Some("N5"), &["dinner", "supper"]),
    lex("ご飯", "ごはん", WordKind::Noun, Some("N5"), &["cooked rice", "meal"]),
    lex("パン", "ぱん", WordKind::Noun, Some("N5"), &["bread"]),
    lex("肉", "にく", WordKind::Noun, Some("N5"), &["meat"]),
    lex("魚", "さかな", WordKind::Noun, Some("N5"), &["fish"]),
    lex("野菜", "やさい", WordKind::Noun, Some("N5"), &["vegetable"]),
    lex("果物", "くだもの", WordKind::Noun, Some("N5"), &["fruit"]),
    lex("卵", "たまご", WordKind::Noun, Some("N5"), &["egg"]),
    lex("牛乳", "ぎゅうにゅう", WordKind::Noun, Some("N5"), &["cow's milk"]),
    lex("水", "みず", WordKind::Noun, Some("N5"), &["water"]),
    lex("お茶", "おちゃ", WordKind::Noun, Some("N5"), &["tea", "green tea"]),
    lex("茶", "ちゃ", WordKind::Noun, Some("N5"), &["tea"]),
    lex("緑茶", "りょくちゃ", WordKind::Noun, Some("N4"), &["green tea"]),
    lex("紅茶", "こうちゃ", WordKind::Noun, Some("N5"), &["black tea"]),
    lex("コーヒー", "こーひー", WordKind::Noun, Some("N5"), &["coffee"]),
    lex("お酒", "おさけ", WordKind::Noun, Some("N5"), &["alcohol", "sake"]),
    lex("酒", "さけ", WordKind::Noun, Some("N5"), &["alcohol", "sake"]),
    lex("ラーメン", "らーめん", WordKind::Noun, Some("N5"), &["ramen"]),
    lex("寿司", "すし", WordKind::Noun, Some("N5"), &["sushi"]),
    lex("納豆", "なっとう", WordKind::Noun, Some("N4"), &["natto (fermented soybeans)"]),
    lex("お菓子", "おかし", WordKind::Noun, Some("N5"), &["sweets", "snacks", "confectionery"]),
    lex("料理", "りょうり", WordKind::SuruNoun, Some("N5"), &["cooking", "cuisine", "dish"]),
    lex("味", "あじ", WordKind::Noun, Some("N4"), &["taste", "flavor"]),
    lex("勉強", "べんきょう", WordKind::SuruNoun, Some("N5"), &["study", "studying"]),
    lex("練習", "れんしゅう", WordKind::SuruNoun, Some("N5"), &["practice", "training"]),
    lex("宿題", "しゅくだい", WordKind::Noun, Some("N5"), &["homework"]),
    lex("試験", "しけん", WordKind::Noun, Some("N4"), &["examination", "exam"]),
    lex("テスト", "てすと", WordKind::SuruNoun, Some("N5"), &["test", "quiz"]),
    lex("問題", "もんだい", WordKind::Noun, Some("N5"), &["problem", "question"]),
    lex("質問", "しつもん", WordKind::SuruNoun, Some("N5"), &["question", "inquiry"]),
    lex("答え", "こたえ", WordKind::Noun, Some("N4"), &["answer", "reply"]),
    lex("返事", "へんじ", WordKind::SuruNoun, Some("N4"), &["reply", "response"]),
    lex("仕事", "しごと", WordKind::SuruNoun, Some("N5"), &["work", "job", "occupation"]),
    lex("会議", "かいぎ", WordKind::SuruNoun, Some("N4"), &["meeting", "conference"]),
    lex("予定通り", "よていどおり", WordKind::Noun, Some("N3"), &["as scheduled", "according to plan"]),
    lex("予定", "よてい", WordKind::SuruNoun, Some("N4"), &["plan", "schedule", "arrangement"]),
    lex("計画", "けいかく", WordKind::SuruNoun, Some("N4"), &["plan", "project", "scheme"]),
    lex("約束", "やくそく", WordKind::SuruNoun, Some("N4"), &["promise", "appointment"]),
    lex("規則", "きそく", WordKind::Noun, Some("N4"), &["rule", "regulation"]),
    lex("生活", "せいかつ", WordKind::SuruNoun, Some("N4"), &["daily life", "living"]),
    lex("習慣", "しゅうかん", WordKind::Noun, Some("N4"), &["habit", "custom"]),
    lex("経験", "けいけん", WordKind::SuruNoun, Some("N4"), &["experience"]),
    lex("趣味", "しゅみ", WordKind::Noun, Some("N4"), &["hobby", "interest"]),
    lex("運動", "うんどう", WordKind::SuruNoun, Some("N4"), &["exercise", "physical movement"]),
    lex("旅行", "りょこう", WordKind::SuruNoun, Some("N5"), &["travel", "trip"]),
    lex("散歩", "さんぽ", WordKind::SuruNoun, Some("N5"), &["walk", "stroll"]),
    lex("掃除", "そうじ", WordKind::SuruNoun, Some("N5"), &["cleaning"]),
    lex("洗濯", "せんたく", WordKind::SuruNoun, Some("N5"), &["laundry", "washing"]),
    lex("買い物", "かいもの", WordKind::SuruNoun, Some("N5"), &["shopping"]),
    lex("食事", "しょくじ", WordKind::SuruNoun, Some("N4"), &["meal", "dining"]),
    lex("予約", "よやく", WordKind::SuruNoun, Some("N4"), &["reservation", "booking"]),
    lex("出発", "しゅっぱつ", WordKind::SuruNoun, Some("N4"), &["departure"]),
    lex("到着", "とうちゃく", WordKind::SuruNoun, Some("N3"), &["arrival"]),
    lex("遅刻", "ちこく", WordKind::SuruNoun, Some("N3"), &["lateness", "being late"]),
    lex("残業", "ざんぎょう", WordKind::SuruNoun, Some("N3"), &["overtime work"]),
    lex("休み", "やすみ", WordKind::Noun, Some("N5"), &["rest", "holiday", "break"]),
    lex("休日", "きゅうじつ", WordKind::Noun, Some("N3"), &["holiday", "day off"]),
    lex("挨拶", "あいさつ", WordKind::SuruNoun, Some("N4"), &["greeting", "salutation"]),
    lex("仕方", "しかた", WordKind::Noun, Some("N4"), &["way", "method", "means"]),
    lex("方法", "ほうほう", WordKind::Noun, Some("N3"), &["method", "process", "way"]),
    lex("理由", "りゆう", WordKind::Noun, Some("N4"), &["reason", "cause"]),
    lex("原因", "げんいん", WordKind::Noun, Some("N4"), &["cause", "origin"]),
    lex("結果", "けっか", WordKind::Noun, Some("N3"), &["result", "outcome"]),
    lex("目的", "もくてき", WordKind::Noun, Some("N3"), &["purpose", "goal", "objective"]),
    lex("目標", "もくひょう", WordKind::Noun, Some("N3"), &["target", "goal"]),
    lex("夢", "ゆめ", WordKind::Noun, Some("N4"), &["dream"]),
    lex("希望", "きぼう", WordKind::SuruNoun, Some("N3"), &["hope", "wish"]),
    lex("気持ち", "きもち", WordKind::Noun, Some("N4"), &["feeling", "mood"]),
    lex("心", "こころ", WordKind::Noun, Some("N4"), &["heart", "mind", "spirit"]),
    lex("体", "からだ", WordKind::Noun, Some("N5"), &["body", "health"]),
    lex("頭", "あたま", WordKind::Noun, Some("N5"), &["head", "mind"]),
    lex("目", "め", WordKind::Noun, Some("N5"), &["eye"]),
    lex("耳", "みみ", WordKind::Noun, Some("N5"), &["ear"]),
    lex("口", "くち", WordKind::Noun, Some("N5"), &["mouth"]),
    lex("手", "て", WordKind::Noun, Some("N5"), &["hand"]),
    lex("足", "あし", WordKind::Noun, Some("N5"), &["foot", "leg"]),
    lex("声", "こえ", WordKind::Noun, Some("N5"), &["voice"]),
    lex("顔", "かお", WordKind::Noun, Some("N5"), &["face"]),
    lex("名前", "なまえ", WordKind::Noun, Some("N5"), &["name"]),
    lex("健康", "けんこう", WordKind::NaAdj, Some("N3"), &["health", "healthy"]),
    lex("病気", "びょうき", WordKind::Noun, Some("N5"), &["illness", "sickness"]),
    lex("風邪", "かぜ", WordKind::Noun, Some("N5"), &["common cold"]),
    lex("熱", "ねつ", WordKind::Noun, Some("N4"), &["fever", "heat"]),
    lex("薬", "くすり", WordKind::Noun, Some("N5"), &["medicine"]),
    lex("経済", "けいざい", WordKind::Noun, Some("N4"), &["economy", "economics"]),
    lex("社会", "しゃかい", WordKind::Noun, Some("N4"), &["society"]),
    lex("政治", "せいじ", WordKind::Noun, Some("N4"), &["politics", "government"]),
    lex("文化", "ぶんか", WordKind::Noun, Some("N4"), &["culture"]),
    lex("歴史", "れきし", WordKind::Noun, Some("N4"), &["history"]),
    lex("科学", "かがく", WordKind::Noun, Some("N4"), &["science"]),
    lex("技術", "ぎじゅつ", WordKind::Noun, Some("N4"), &["technology", "skill"]),
    lex("教育", "きょういく", WordKind::SuruNoun, Some("N4"), &["education"]),
    lex("情報", "じょうほう", WordKind::Noun, Some("N3"), &["information"]),
    lex("物価", "ぶっか", WordKind::Noun, Some("N2"), &["prices of goods", "cost of living"]),
    lex("価格", "かかく", WordKind::Noun, Some("N3"), &["price", "value"]),
    lex("値段", "ねだん", WordKind::Noun, Some("N4"), &["price", "cost"]),
    lex("試合", "しあい", WordKind::SuruNoun, Some("N4"), &["match", "game", "bout"]),
    lex("応援", "おうえん", WordKind::SuruNoun, Some("N3"), &["support", "cheering"]),
    lex("感謝", "かんしゃ", WordKind::SuruNoun, Some("N3"), &["gratitude", "thanks"]),
    lex("合格", "ごうかく", WordKind::SuruNoun, Some("N3"), &["passing an exam", "success"]),
    lex("変更", "へんこう", WordKind::SuruNoun, Some("N3"), &["change", "modification"]),
    lex("留学", "りゅうがく", WordKind::SuruNoun, Some("N4"), &["studying abroad"]),
    lex("上達", "じょうたつ", WordKind::SuruNoun, Some("N2"), &["improvement", "progress"]),
    lex("説明", "せつめい", WordKind::SuruNoun, Some("N4"), &["explanation"]),
    lex("準備", "じゅんび", WordKind::SuruNoun, Some("N4"), &["preparation"]),
    lex("確認", "かくにん", WordKind::SuruNoun, Some("N3"), &["confirmation", "verification"]),
    lex("参加", "さんか", WordKind::SuruNoun, Some("N3"), &["participation"]),
    lex("紹介", "しょうかい", WordKind::SuruNoun, Some("N4"), &["introduction"]),
    lex("案内", "あんない", WordKind::SuruNoun, Some("N4"), &["guidance", "showing around"]),
    lex("利用", "りよう", WordKind::SuruNoun, Some("N4"), &["use", "utilization"]),
    lex("努力", "どりょく", WordKind::SuruNoun, Some("N3"), &["effort", "endeavor"]),
    lex("協力", "きょうりょく", WordKind::SuruNoun, Some("N3"), &["cooperation"]),
    lex("理解", "りかい", WordKind::SuruNoun, Some("N3"), &["understanding", "comprehension"]),
    lex("連絡", "れんらく", WordKind::SuruNoun, Some("N4"), &["contact", "communication"]),
    lex("相談", "そうだん", WordKind::SuruNoun, Some("N4"), &["consultation", "discussion"]),
    lex("報告", "ほうこく", WordKind::SuruNoun, Some("N3"), &["report"]),
    lex("発表", "はっぴょう", WordKind::SuruNoun, Some("N3"), &["announcement", "presentation"]),
    lex("研究", "けんきゅう", WordKind::SuruNoun, Some("N4"), &["research", "study"]),
    lex("結婚", "けっこん", WordKind::SuruNoun, Some("N5"), &["marriage"]),
    lex("卒業", "そつぎょう", WordKind::SuruNoun, Some("N4"), &["graduation"]),
    lex("心配", "しんぱい", WordKind::SuruNoun, Some("N4"), &["worry", "concern"]),
    lex("安心", "あんしん", WordKind::SuruNoun, Some("N4"), &["relief", "peace of mind"]),
    lex("注意", "ちゅうい", WordKind::SuruNoun, Some("N4"), &["caution", "attention"]),
    lex("成功", "せいこう", WordKind::SuruNoun, Some("N3"), &["success"]),
    lex("失敗", "しっぱい", WordKind::SuruNoun, Some("N4"), &["failure", "mistake"]),
    lex("解決", "かいけつ", WordKind::SuruNoun, Some("N3"), &["solution", "resolution"]),
    lex("荷物", "にもつ", WordKind::Noun, Some("N5"), &["luggage", "baggage"]),
    lex("全部", "ぜんぶ", WordKind::Noun, Some("N5"), &["all", "entire", "everything"]),
    lex("半分", "はんぶん", WordKind::Noun, Some("N5"), &["half"]),
    lex("本当", "ほんとう", WordKind::Noun, Some("N5"), &["truth", "reality", "really"]),
    lex("早め", "はやめ", WordKind::Noun, Some("N3"), &["a little early", "on the early side"]),
    lex("夜更かし", "よふかし", WordKind::SuruNoun, Some("N3"), &["staying up late"]),
    lex("物", "もの", WordKind::Noun, Some("N5"), &["thing", "object"]),
    lex("もの", "もの", WordKind::Noun, Some("N5"), &["thing", "object"]),
    lex("事", "こと", WordKind::Noun, Some("N5"), &["thing", "matter", "abstract fact"]),
    lex("こと", "こと", WordKind::Noun, Some("N5"), &["thing", "matter", "nominalizer"]),
    lex("ため", "ため", WordKind::Noun, Some("N4"), &["sake", "benefit", "purpose"]),

    // ==================== Verbs (Irregular, Ichidan, Godan) ====================
    lex("する", "する", WordKind::SuruVerb, Some("N5"), &["to do", "to make"]),
    lex("来る", "くる", WordKind::KuruVerb, Some("N5"), &["to come"]),
    lex("くる", "くる", WordKind::KuruVerb, Some("N5"), &["to come", "to become (auxiliary)"]),
    lex("ある", "ある", WordKind::Godan, Some("N5"), &["to exist (inanimate)", "to have"]),
    lex("いる", "いる", WordKind::Ichidan, Some("N5"), &["to exist (animate)", "ongoing state (~ている)"]),
    lex("なる", "なる", WordKind::Godan, Some("N5"), &["to become"]),
    lex("行く", "いく", WordKind::Godan, Some("N5"), &["to go"]),
    lex("いく", "いく", WordKind::Godan, Some("N5"), &["to go", "to continue (auxiliary)"]),
    lex("行う", "おこなう", WordKind::Godan, Some("N3"), &["to perform", "to carry out", "to hold (an event)"]),
    lex("帰る", "かえる", WordKind::Godan, Some("N5"), &["to return home"]),
    lex("出る", "でる", WordKind::Ichidan, Some("N5"), &["to leave", "to exit", "to attend"]),
    lex("入る", "はいる", WordKind::Godan, Some("N5"), &["to enter"]),
    lex("入れる", "いれる", WordKind::Ichidan, Some("N5"), &["to put in", "to insert"]),
    lex("出す", "だす", WordKind::Godan, Some("N5"), &["to take out", "to submit"]),
    lex("食べる", "たべる", WordKind::Ichidan, Some("N5"), &["to eat"]),
    lex("飲む", "のむ", WordKind::Godan, Some("N5"), &["to drink"]),
    lex("見る", "みる", WordKind::Ichidan, Some("N5"), &["to see", "to watch", "to look at"]),
    lex("みる", "みる", WordKind::Ichidan, Some("N4"), &["to try doing (~てみる)", "to see"]),
    lex("聞く", "きく", WordKind::Godan, Some("N5"), &["to listen", "to hear", "to ask"]),
    lex("話す", "はなす", WordKind::Godan, Some("N5"), &["to speak", "to talk"]),
    lex("言う", "いう", WordKind::Godan, Some("N5"), &["to say", "to tell"]),
    lex("読む", "よむ", WordKind::Godan, Some("N5"), &["to read"]),
    lex("書く", "かく", WordKind::Godan, Some("N5"), &["to write"]),
    lex("買う", "かう", WordKind::Godan, Some("N5"), &["to buy"]),
    lex("売る", "うる", WordKind::Godan, Some("N5"), &["to sell"]),
    lex("作る", "つくる", WordKind::Godan, Some("N5"), &["to make", "to create"]),
    lex("使う", "つかう", WordKind::Godan, Some("N5"), &["to use"]),
    lex("持つ", "もつ", WordKind::Godan, Some("N5"), &["to hold", "to possess"]),
    lex("待つ", "まつ", WordKind::Godan, Some("N5"), &["to wait"]),
    lex("取る", "とる", WordKind::Godan, Some("N5"), &["to take", "to pick up"]),
    lex("撮る", "とる", WordKind::Godan, Some("N5"), &["to take (a photo)"]),
    lex("渡す", "わたす", WordKind::Godan, Some("N5"), &["to hand over"]),
    lex("送る", "おくる", WordKind::Godan, Some("N4"), &["to send"]),
    lex("届く", "とどく", WordKind::Godan, Some("N4"), &["to reach", "to arrive"]),
    lex("届ける", "とどける", WordKind::Ichidan, Some("N4"), &["to deliver"]),
    lex("開ける", "あける", WordKind::Ichidan, Some("N5"), &["to open (transitive)"]),
    lex("開く", "あく", WordKind::Godan, Some("N5"), &["to open (intransitive)"]),
    lex("閉める", "しめる", WordKind::Ichidan, Some("N5"), &["to close (transitive)"]),
    lex("閉まる", "しまる", WordKind::Godan, Some("N5"), &["to close (intransitive)"]),
    lex("始まる", "はじまる", WordKind::Godan, Some("N5"), &["to begin (intransitive)"]),
    lex("始める", "はじめる", WordKind::Ichidan, Some("N4"), &["to start (transitive)"]),
    lex("終わる", "おわる", WordKind::Godan, Some("N5"), &["to end", "to finish"]),
    lex("起きる", "おきる", WordKind::Ichidan, Some("N5"), &["to wake up", "to happen"]),
    lex("寝る", "ねる", WordKind::Ichidan, Some("N5"), &["to sleep", "to go to bed"]),
    lex("休む", "やすむ", WordKind::Godan, Some("N5"), &["to rest", "to take a day off"]),
    lex("働く", "はたらく", WordKind::Godan, Some("N5"), &["to work"]),
    lex("遊ぶ", "あそぶ", WordKind::Godan, Some("N5"), &["to play", "to hang out"]),
    lex("歩く", "あるく", WordKind::Godan, Some("N5"), &["to walk"]),
    lex("走る", "はしる", WordKind::Godan, Some("N5"), &["to run"]),
    lex("泳ぐ", "およぐ", WordKind::Godan, Some("N5"), &["to swim"]),
    lex("飛ぶ", "とぶ", WordKind::Godan, Some("N5"), &["to fly", "to jump"]),
    lex("登る", "のぼる", WordKind::Godan, Some("N5"), &["to climb", "to ascend"]),
    lex("降りる", "おりる", WordKind::Ichidan, Some("N5"), &["to get off", "to descend"]),
    lex("乗る", "のる", WordKind::Godan, Some("N5"), &["to ride", "to board"]),
    lex("乗り換える", "のりかえる", WordKind::Ichidan, Some("N4"), &["to transfer (trains/buses)"]),
    lex("着く", "つく", WordKind::Godan, Some("N5"), &["to arrive at"]),
    lex("着る", "きる", WordKind::Ichidan, Some("N5"), &["to wear (upper body)"]),
    lex("脱ぐ", "ぬぐ", WordKind::Godan, Some("N5"), &["to take off (clothes/shoes)"]),
    lex("洗う", "あらう", WordKind::Godan, Some("N5"), &["to wash"]),
    lex("知る", "しる", WordKind::Godan, Some("N5"), &["to know"]),
    lex("分かる", "わかる", WordKind::Godan, Some("N5"), &["to understand", "to be clear"]),
    lex("わかる", "わかる", WordKind::Godan, Some("N5"), &["to understand"]),
    lex("覚える", "おぼえる", WordKind::Ichidan, Some("N5"), &["to memorize", "to remember"]),
    lex("忘れる", "わすれる", WordKind::Ichidan, Some("N5"), &["to forget"]),
    lex("考える", "かんがえる", WordKind::Ichidan, Some("N4"), &["to think", "to consider"]),
    lex("思う", "おもう", WordKind::Godan, Some("N4"), &["to think", "to feel"]),
    lex("信じる", "しんじる", WordKind::Ichidan, Some("N3"), &["to believe", "to trust"]),
    lex("決める", "きめる", WordKind::Ichidan, Some("N4"), &["to decide"]),
    lex("決まる", "きまる", WordKind::Godan, Some("N4"), &["to be decided"]),
    lex("選ぶ", "えらぶ", WordKind::Godan, Some("N4"), &["to choose", "to select"]),
    lex("探す", "さがす", WordKind::Godan, Some("N4"), &["to search for", "to look for"]),
    lex("見つける", "みつける", WordKind::Ichidan, Some("N4"), &["to find"]),
    lex("見つかる", "みつかる", WordKind::Godan, Some("N4"), &["to be found"]),
    lex("教える", "おしえる", WordKind::Ichidan, Some("N5"), &["to teach", "to tell"]),
    lex("習う", "ならう", WordKind::Godan, Some("N5"), &["to learn (from someone)"]),
    lex("学ぶ", "まなぶ", WordKind::Godan, Some("N3"), &["to study", "to learn"]),
    lex("伝える", "つたえる", WordKind::Ichidan, Some("N4"), &["to convey", "to report"]),
    lex("手伝う", "てつだう", WordKind::Godan, Some("N4"), &["to help", "to assist"]),
    lex("助ける", "たすける", WordKind::Ichidan, Some("N3"), &["to save", "to rescue"]),
    lex("守る", "まもる", WordKind::Godan, Some("N3"), &["to protect", "to obey"]),
    lex("変える", "かえる", WordKind::Ichidan, Some("N4"), &["to change (transitive)"]),
    lex("変わる", "かわる", WordKind::Godan, Some("N4"), &["to change (intransitive)"]),
    lex("直す", "なおす", WordKind::Godan, Some("N4"), &["to fix", "to correct"]),
    lex("直る", "なおる", WordKind::Godan, Some("N4"), &["to be fixed"]),
    lex("治る", "なおる", WordKind::Godan, Some("N4"), &["to recover", "to heal"]),
    lex("壊す", "こわす", WordKind::Godan, Some("N4"), &["to break (transitive)"]),
    lex("壊れる", "こわれる", WordKind::Ichidan, Some("N4"), &["to break (intransitive)"]),
    lex("切る", "きる", WordKind::Godan, Some("N5"), &["to cut"]),
    lex("落とす", "おとす", WordKind::Godan, Some("N4"), &["to drop"]),
    lex("落ちる", "おちる", WordKind::Ichidan, Some("N4"), &["to fall"]),
    lex("上げる", "あげる", WordKind::Ichidan, Some("N5"), &["to raise", "to give"]),
    lex("上がる", "あがる", WordKind::Godan, Some("N4"), &["to rise", "to go up"]),
    lex("下げる", "さげる", WordKind::Ichidan, Some("N4"), &["to lower"]),
    lex("下がる", "さがる", WordKind::Godan, Some("N4"), &["to go down", "to drop"]),
    lex("増える", "ふえる", WordKind::Ichidan, Some("N4"), &["to increase (intransitive)"]),
    lex("増やす", "ふやす", WordKind::Godan, Some("N3"), &["to increase (transitive)"]),
    lex("減る", "へる", WordKind::Godan, Some("N3"), &["to decrease"]),
    lex("減らす", "へらす", WordKind::Godan, Some("N3"), &["to reduce"]),
    lex("続ける", "つづける", WordKind::Ichidan, Some("N4"), &["to continue (transitive)"]),
    lex("続く", "つづく", WordKind::Godan, Some("N4"), &["to continue (intransitive)"]),
    lex("止める", "とめる", WordKind::Ichidan, Some("N4"), &["to stop (transitive)"]),
    lex("止まる", "とまる", WordKind::Godan, Some("N5"), &["to stop (intransitive)"]),
    lex("やめる", "やめる", WordKind::Ichidan, Some("N4"), &["to quit", "to stop doing"]),
    lex("頑張る", "がんばる", WordKind::Godan, Some("N4"), &["to do one's best", "to persevere"]),
    lex("慣れる", "なれる", WordKind::Ichidan, Some("N4"), &["to get used to"]),
    lex("遅れる", "おくれる", WordKind::Ichidan, Some("N4"), &["to be late", "to be delayed"]),
    lex("急ぐ", "いそぐ", WordKind::Godan, Some("N4"), &["to hurry"]),
    lex("近づく", "ちかづく", WordKind::Godan, Some("N3"), &["to approach", "to draw near"]),
    lex("離れる", "はなれる", WordKind::Ichidan, Some("N3"), &["to separate", "to move away"]),
    lex("降る", "ふる", WordKind::Godan, Some("N5"), &["to fall (rain/snow)"]),
    lex("晴れる", "はれる", WordKind::Ichidan, Some("N5"), &["to clear up (weather)"]),
    lex("曇る", "くもる", WordKind::Godan, Some("N5"), &["to become cloudy"]),
    lex("吹く", "ふく", WordKind::Godan, Some("N5"), &["to blow (wind)"]),
    lex("咲く", "さく", WordKind::Godan, Some("N5"), &["to bloom"]),
    lex("笑う", "わらう", WordKind::Godan, Some("N4"), &["to laugh", "to smile"]),
    lex("泣く", "なく", WordKind::Godan, Some("N4"), &["to cry"]),
    lex("怒る", "おこる", WordKind::Godan, Some("N4"), &["to get angry", "to scold"]),
    lex("喜ぶ", "よろこぶ", WordKind::Godan, Some("N4"), &["to be delighted", "to rejoice"]),
    lex("驚く", "おどろく", WordKind::Godan, Some("N4"), &["to be surprised"]),
    lex("困る", "こまる", WordKind::Godan, Some("N5"), &["to be troubled"]),
    lex("疲れる", "つかれる", WordKind::Ichidan, Some("N5"), &["to get tired"]),
    lex("住む", "すむ", WordKind::Godan, Some("N5"), &["to live", "to reside"]),
    lex("通う", "かよう", WordKind::Godan, Some("N4"), &["to commute", "to attend regularly"]),
    lex("出かける", "でかける", WordKind::Ichidan, Some("N5"), &["to go out"]),
    lex("集まる", "あつまる", WordKind::Godan, Some("N4"), &["to gather (intransitive)"]),
    lex("集める", "あつめる", WordKind::Ichidan, Some("N4"), &["to collect", "to gather"]),
    lex("並べる", "ならべる", WordKind::Ichidan, Some("N5"), &["to line up (transitive)"]),
    lex("並ぶ", "ならぶ", WordKind::Godan, Some("N5"), &["to line up (intransitive)"]),
    lex("置く", "おく", WordKind::Godan, Some("N5"), &["to place", "to put"]),
    lex("おく", "おく", WordKind::Godan, Some("N4"), &["to do in advance (~ておく)", "to leave as is"]),
    lex("しまう", "しまう", WordKind::Godan, Some("N4"), &["to finish completely / accidentally (~てしまう)"]),
    lex("残る", "のこる", WordKind::Godan, Some("N4"), &["to remain", "to be left over"]),
    lex("残す", "のこす", WordKind::Godan, Some("N3"), &["to leave behind"]),
    lex("足りる", "たりる", WordKind::Ichidan, Some("N4"), &["to be sufficient", "to be enough"]),
    lex("勝つ", "かつ", WordKind::Godan, Some("N4"), &["to win"]),
    lex("負ける", "まける", WordKind::Ichidan, Some("N4"), &["to lose", "to be defeated"]),
    lex("乱れる", "みだれる", WordKind::Ichidan, Some("N2"), &["to be disrupted", "to be thrown into disorder"]),
    lex("堪える", "たえる", WordKind::Ichidan, Some("N1"), &["to endure", "to be worthy of"]),
    lex("耐える", "たえる", WordKind::Ichidan, Some("N2"), &["to withstand", "to bear"]),
    lex("受ける", "うける", WordKind::Ichidan, Some("N4"), &["to receive", "to take (an exam)"]),
    lex("渡る", "わたる", WordKind::Godan, Some("N5"), &["to cross over"]),
    lex("曲がる", "まがる", WordKind::Godan, Some("N5"), &["to turn", "to bend"]),
    lex("進む", "すすむ", WordKind::Godan, Some("N4"), &["to advance", "to move forward"]),
    lex("戻る", "もどる", WordKind::Godan, Some("N4"), &["to turn back", "to return"]),
    lex("返す", "かえす", WordKind::Godan, Some("N5"), &["to return (something)"]),
    lex("借りる", "かりる", WordKind::Ichidan, Some("N5"), &["to borrow"]),
    lex("貸す", "かす", WordKind::Godan, Some("N5"), &["to lend"]),
    lex("払う", "はらう", WordKind::Godan, Some("N4"), &["to pay"]),
    lex("違う", "ちがう", WordKind::Godan, Some("N5"), &["to differ", "to be wrong"]),
    lex("比べる", "くらべる", WordKind::Ichidan, Some("N4"), &["to compare"]),
    lex("調べる", "しらべる", WordKind::Ichidan, Some("N4"), &["to investigate", "to look up"]),
    lex("できる", "できる", WordKind::Ichidan, Some("N5"), &["to be able to", "can do", "to be completed"]),
    lex("出来る", "できる", WordKind::Ichidan, Some("N5"), &["to be able to", "can do"]),
    lex("くれる", "くれる", WordKind::Ichidan, Some("N4"), &["to give (to speaker)"]),
    lex("あげる", "あげる", WordKind::Ichidan, Some("N5"), &["to give"]),
    lex("もらう", "もらう", WordKind::Godan, Some("N5"), &["to receive"]),
    lex("消す", "けす", WordKind::Godan, Some("N5"), &["to turn off", "to erase"]),
    lex("消える", "きえる", WordKind::Ichidan, Some("N4"), &["to disappear", "to go out"]),
    lex("かける", "かける", WordKind::Ichidan, Some("N5"), &["to hang", "to call", "to spend"]),
    lex("かかる", "かかる", WordKind::Godan, Some("N5"), &["to take (time/money)", "to hang"]),

    // ==================== い-Adjectives ====================
    lex("ない", "ない", WordKind::IAdj, Some("N5"), &["not exist", "nonexistent"]),
    lex("いい", "いい", WordKind::IAdj, Some("N5"), &["good", "fine"]),
    lex("良い", "よい", WordKind::IAdj, Some("N5"), &["good", "fine"]),
    lex("よい", "よい", WordKind::IAdj, Some("N5"), &["good", "fine"]),
    lex("悪い", "わるい", WordKind::IAdj, Some("N5"), &["bad", "poor"]),
    lex("大きい", "おおきい", WordKind::IAdj, Some("N5"), &["big", "large"]),
    lex("小さい", "ちいさい", WordKind::IAdj, Some("N5"), &["small", "little"]),
    lex("高い", "たかい", WordKind::IAdj, Some("N5"), &["high", "tall", "expensive"]),
    lex("低い", "ひくい", WordKind::IAdj, Some("N5"), &["low", "short"]),
    lex("安い", "やすい", WordKind::IAdj, Some("N5"), &["cheap", "inexpensive"]),
    lex("新しい", "あたらしい", WordKind::IAdj, Some("N5"), &["new", "fresh"]),
    lex("古い", "ふるい", WordKind::IAdj, Some("N5"), &["old (things)"]),
    lex("長い", "ながい", WordKind::IAdj, Some("N5"), &["long"]),
    lex("短い", "みじかい", WordKind::IAdj, Some("N5"), &["short"]),
    lex("広い", "ひろい", WordKind::IAdj, Some("N5"), &["spacious", "wide"]),
    lex("狭い", "せまい", WordKind::IAdj, Some("N5"), &["narrow", "cramped"]),
    lex("多い", "おおい", WordKind::IAdj, Some("N5"), &["many", "much"]),
    lex("少ない", "すくない", WordKind::IAdj, Some("N5"), &["few", "little"]),
    lex("早い", "はやい", WordKind::IAdj, Some("N5"), &["early", "fast"]),
    lex("速い", "はやい", WordKind::IAdj, Some("N5"), &["fast", "quick"]),
    lex("遅い", "おそい", WordKind::IAdj, Some("N5"), &["slow", "late"]),
    lex("近い", "ちかい", WordKind::IAdj, Some("N5"), &["near", "close"]),
    lex("遠い", "とおい", WordKind::IAdj, Some("N5"), &["far", "distant"]),
    lex("強い", "つよい", WordKind::IAdj, Some("N5"), &["strong"]),
    lex("弱い", "よわい", WordKind::IAdj, Some("N5"), &["weak"]),
    lex("重い", "おもい", WordKind::IAdj, Some("N5"), &["heavy"]),
    lex("軽い", "かるい", WordKind::IAdj, Some("N5"), &["light (weight)"]),
    lex("暑い", "あつい", WordKind::IAdj, Some("N5"), &["hot (weather)"]),
    lex("熱い", "あつい", WordKind::IAdj, Some("N5"), &["hot (to touch)"]),
    lex("寒い", "さむい", WordKind::IAdj, Some("N5"), &["cold (weather)"]),
    lex("冷たい", "つめたい", WordKind::IAdj, Some("N5"), &["cold (to touch)"]),
    lex("温かい", "あたたかい", WordKind::IAdj, Some("N5"), &["warm (objects/kindness)"]),
    lex("暖かい", "あたたかい", WordKind::IAdj, Some("N5"), &["warm (weather)"]),
    lex("涼しい", "すずしい", WordKind::IAdj, Some("N5"), &["cool", "refreshing"]),
    lex("明るい", "あかるい", WordKind::IAdj, Some("N5"), &["bright", "cheerful"]),
    lex("暗い", "くらい", WordKind::IAdj, Some("N5"), &["dark", "gloomy"]),
    lex("白い", "しろい", WordKind::IAdj, Some("N5"), &["white"]),
    lex("黒い", "くろい", WordKind::IAdj, Some("N5"), &["black"]),
    lex("赤い", "あかい", WordKind::IAdj, Some("N5"), &["red"]),
    lex("青い", "あおい", WordKind::IAdj, Some("N5"), &["blue"]),
    lex("美しい", "うつくしい", WordKind::IAdj, Some("N4"), &["beautiful", "lovely"]),
    lex("可愛い", "かわいい", WordKind::IAdj, Some("N5"), &["cute", "adorable"]),
    lex("かわいい", "かわいい", WordKind::IAdj, Some("N5"), &["cute", "adorable"]),
    lex("楽しい", "たのしい", WordKind::IAdj, Some("N5"), &["fun", "enjoyable"]),
    lex("嬉しい", "うれしい", WordKind::IAdj, Some("N4"), &["happy", "glad"]),
    lex("悲しい", "かなしい", WordKind::IAdj, Some("N4"), &["sad"]),
    lex("寂しい", "さびしい", WordKind::IAdj, Some("N4"), &["lonely"]),
    lex("怖い", "こわい", WordKind::IAdj, Some("N4"), &["scary", "frightening"]),
    lex("痛い", "いたい", WordKind::IAdj, Some("N5"), &["painful", "sore"]),
    lex("甘い", "あまい", WordKind::IAdj, Some("N5"), &["sweet"]),
    lex("辛い", "からい", WordKind::IAdj, Some("N5"), &["spicy", "hot"]),
    lex("美味しい", "おいしい", WordKind::IAdj, Some("N5"), &["delicious", "tasty"]),
    lex("おいしい", "おいしい", WordKind::IAdj, Some("N5"), &["delicious", "tasty"]),
    lex("難しい", "むずかしい", WordKind::IAdj, Some("N5"), &["difficult", "hard"]),
    lex("易しい", "やさしい", WordKind::IAdj, Some("N5"), &["easy", "simple"]),
    lex("優しい", "やさしい", WordKind::IAdj, Some("N4"), &["kind", "gentle"]),
    lex("厳しい", "きびしい", WordKind::IAdj, Some("N4"), &["strict", "severe"]),
    lex("忙しい", "いそがしい", WordKind::IAdj, Some("N5"), &["busy"]),
    lex("面白い", "おもしろい", WordKind::IAdj, Some("N5"), &["interesting", "funny"]),
    lex("つまらない", "つまらない", WordKind::IAdj, Some("N5"), &["boring", "dull"]),
    lex("眠い", "ねむい", WordKind::IAdj, Some("N4"), &["sleepy"]),
    lex("正しい", "ただしい", WordKind::IAdj, Some("N4"), &["correct", "right"]),
    lex("素晴らしい", "すばらしい", WordKind::IAdj, Some("N4"), &["wonderful", "splendid"]),
    lex("危ない", "あぶない", WordKind::IAdj, Some("N5"), &["dangerous"]),
    lex("若い", "わかい", WordKind::IAdj, Some("N5"), &["young"]),
    lex("欲しい", "ほしい", WordKind::IAdj, Some("N5"), &["wanted", "desired"]),
    lex("ほしい", "ほしい", WordKind::IAdj, Some("N5"), &["wanted", "desired"]),
    lex("よろしい", "よろしい", WordKind::IAdj, Some("N4"), &["good", "all right (formal)"]),

    // ==================== な-Adjectives ====================
    lex("静か", "しずか", WordKind::NaAdj, Some("N5"), &["quiet", "peaceful"]),
    lex("賑やか", "にぎやか", WordKind::NaAdj, Some("N5"), &["lively", "bustling"]),
    lex("綺麗", "きれい", WordKind::NaAdj, Some("N5"), &["pretty", "clean", "beautiful"]),
    lex("きれい", "きれい", WordKind::NaAdj, Some("N5"), &["pretty", "clean"]),
    lex("元気", "げんき", WordKind::NaAdj, Some("N5"), &["energetic", "healthy", "well"]),
    lex("暇", "ひま", WordKind::NaAdj, Some("N5"), &["free (time)", "unoccupied"]),
    lex("便利", "べんり", WordKind::NaAdj, Some("N5"), &["convenient", "handy"]),
    lex("不便", "ふべん", WordKind::NaAdj, Some("N4"), &["inconvenient"]),
    lex("有名", "ゆうめい", WordKind::NaAdj, Some("N5"), &["famous"]),
    lex("親切", "しんせつ", WordKind::NaAdj, Some("N4"), &["kind", "helpful"]),
    lex("丁寧", "ていねい", WordKind::NaAdj, Some("N4"), &["polite", "careful"]),
    lex("大好き", "だいすき", WordKind::NaAdj, Some("N5"), &["love", "very fond of"]),
    lex("好き", "すき", WordKind::NaAdj, Some("N5"), &["liked", "favorite"]),
    lex("嫌い", "きらい", WordKind::NaAdj, Some("N5"), &["disliked", "hated"]),
    lex("上手", "じょうず", WordKind::NaAdj, Some("N5"), &["skillful", "good at"]),
    lex("下手", "へた", WordKind::NaAdj, Some("N5"), &["unskillful", "poor at"]),
    lex("得意", "とくい", WordKind::NaAdj, Some("N3"), &["strong point", "good at"]),
    lex("苦手", "にがて", WordKind::NaAdj, Some("N3"), &["weak at", "not fond of"]),
    lex("大切", "たいせつ", WordKind::NaAdj, Some("N5"), &["important", "precious"]),
    lex("大事", "だいじ", WordKind::NaAdj, Some("N4"), &["important", "valuable"]),
    lex("必要", "ひつよう", WordKind::NaAdj, Some("N4"), &["necessary", "essential"]),
    lex("重要", "じゅうよう", WordKind::NaAdj, Some("N3"), &["important", "significant"]),
    lex("大変", "たいへん", WordKind::NaAdj, Some("N5"), &["tough", "hard", "very"]),
    lex("簡単", "かんたん", WordKind::NaAdj, Some("N4"), &["simple", "easy"]),
    lex("複雑", "ふくざつ", WordKind::NaAdj, Some("N4"), &["complex", "complicated"]),
    lex("安全", "あんぜん", WordKind::NaAdj, Some("N4"), &["safe", "safety"]),
    lex("危険", "きけん", WordKind::NaAdj, Some("N4"), &["dangerous", "danger"]),
    lex("自由", "じゆう", WordKind::NaAdj, Some("N4"), &["free", "freedom"]),
    lex("無理", "むり", WordKind::NaAdj, Some("N4"), &["impossible", "unreasonable", "overdoing"]),
    lex("素敵", "すてき", WordKind::NaAdj, Some("N4"), &["lovely", "wonderful"]),
    lex("立派", "りっぱ", WordKind::NaAdj, Some("N4"), &["splendid", "admirable"]),
    lex("丈夫", "じょうぶ", WordKind::NaAdj, Some("N5"), &["sturdy", "healthy"]),
    lex("真面目", "まじめ", WordKind::NaAdj, Some("N4"), &["serious", "earnest"]),
    lex("特別", "とくべつ", WordKind::NaAdj, Some("N4"), &["special"]),
    lex("普通", "ふつう", WordKind::NaAdj, Some("N4"), &["ordinary", "normal"]),

    // ==================== Adverbs ====================
    lex("少しずつ", "すこしずつ", WordKind::Adverb, Some("N4"), &["little by little", "gradually"]),
    lex("少し", "すこし", WordKind::Adverb, Some("N5"), &["a little", "a bit"]),
    lex("一緒に", "いっしょに", WordKind::Adverb, Some("N5"), &["together"]),
    lex("一緒", "いっしょ", WordKind::Noun, Some("N5"), &["together", "same"]),
    lex("とても", "とても", WordKind::Adverb, Some("N5"), &["very", "extremely"]),
    lex("すごく", "すごく", WordKind::Adverb, Some("N4"), &["terribly", "amazingly", "very"]),
    lex("かなり", "かなり", WordKind::Adverb, Some("N3"), &["considerably", "fairly"]),
    lex("ちょっと", "ちょっと", WordKind::Adverb, Some("N5"), &["a little", "just a moment"]),
    lex("あまり", "あまり", WordKind::Adverb, Some("N5"), &["not very (with negative)", "excessively"]),
    lex("全然", "ぜんぜん", WordKind::Adverb, Some("N5"), &["not at all (with negative)"]),
    lex("まったく", "まったく", WordKind::Adverb, Some("N3"), &["entirely", "not at all"]),
    lex("本当に", "ほんとうに", WordKind::Adverb, Some("N5"), &["really", "truly"]),
    lex("確かに", "たしかに", WordKind::Adverb, Some("N3"), &["surely", "certainly"]),
    lex("きっと", "きっと", WordKind::Adverb, Some("N4"), &["surely", "undoubtedly"]),
    lex("たぶん", "たぶん", WordKind::Adverb, Some("N4"), &["probably", "perhaps"]),
    lex("多分", "たぶん", WordKind::Adverb, Some("N4"), &["probably", "perhaps"]),
    lex("ぜひ", "ぜひ", WordKind::Adverb, Some("N4"), &["by all means", "certainly"]),
    lex("必ず", "かならず", WordKind::Adverb, Some("N4"), &["without fail", "certainly"]),
    lex("絶対", "ぜったい", WordKind::Adverb, Some("N3"), &["absolutely", "definitely"]),
    lex("ちょうど", "ちょうど", WordKind::Adverb, Some("N5"), &["just", "exactly"]),
    lex("やっと", "やっと", WordKind::Adverb, Some("N4"), &["at last", "finally"]),
    lex("ようやく", "ようやく", WordKind::Adverb, Some("N3"), &["finally", "at last"]),
    lex("すぐに", "すぐに", WordKind::Adverb, Some("N5"), &["immediately", "soon"]),
    lex("すぐ", "すぐ", WordKind::Adverb, Some("N5"), &["immediately", "right away"]),
    lex("もうすぐ", "もうすぐ", WordKind::Adverb, Some("N4"), &["soon", "before long"]),
    lex("もう一度", "もういちど", WordKind::Adverb, Some("N5"), &["once more", "again"]),
    lex("もう", "もう", WordKind::Adverb, Some("N5"), &["already", "anymore", "soon"]),
    lex("まだ", "まだ", WordKind::Adverb, Some("N5"), &["still", "not yet"]),
    lex("また", "また", WordKind::Adverb, Some("N5"), &["again", "also"]),
    lex("よく", "よく", WordKind::Adverb, Some("N5"), &["often", "well"]),
    lex("いつも", "いつも", WordKind::Adverb, Some("N5"), &["always"]),
    lex("たいてい", "たいてい", WordKind::Adverb, Some("N5"), &["usually", "mostly"]),
    lex("時々", "ときどき", WordKind::Adverb, Some("N5"), &["sometimes"]),
    lex("ときどき", "ときどき", WordKind::Adverb, Some("N5"), &["sometimes"]),
    lex("たまに", "たまに", WordKind::Adverb, Some("N3"), &["occasionally"]),
    lex("ほとんど", "ほとんど", WordKind::Adverb, Some("N4"), &["almost", "mostly"]),
    lex("すべて", "すべて", WordKind::Noun, Some("N3"), &["all", "everything"]),
    lex("全て", "すべて", WordKind::Noun, Some("N3"), &["all", "everything"]),
    lex("だいたい", "だいたい", WordKind::Adverb, Some("N4"), &["roughly", "mostly"]),
    lex("どんどん", "どんどん", WordKind::Adverb, Some("N4"), &["rapidly", "steadily"]),
    lex("だんだん", "だんだん", WordKind::Adverb, Some("N5"), &["gradually"]),
    lex("ゆっくり", "ゆっくり", WordKind::Adverb, Some("N5"), &["slowly", "at ease"]),
    lex("はっきり", "はっきり", WordKind::Adverb, Some("N4"), &["clearly", "distinctly"]),
    lex("しっかり", "しっかり", WordKind::Adverb, Some("N4"), &["firmly", "properly"]),
    lex("ずっと", "ずっと", WordKind::Adverb, Some("N4"), &["all along", "much more"]),
    lex("もっと", "もっと", WordKind::Adverb, Some("N5"), &["more"]),
    lex("一番", "いちばん", WordKind::Adverb, Some("N5"), &["most", "number one"]),
    lex("特に", "とくに", WordKind::Adverb, Some("N4"), &["especially", "particularly"]),
    lex("別に", "べつに", WordKind::Adverb, Some("N4"), &["not particularly"]),
    lex("決して", "けっして", WordKind::Adverb, Some("N3"), &["never", "by no means"]),
    lex("もちろん", "もちろん", WordKind::Adverb, Some("N4"), &["of course"]),
    lex("やはり", "やはり", WordKind::Adverb, Some("N4"), &["as expected", "after all"]),
    lex("やっぱり", "やっぱり", WordKind::Adverb, Some("N4"), &["as expected (casual)"]),
    lex("ついに", "ついに", WordKind::Adverb, Some("N3"), &["finally", "at last"]),
    lex("つい", "つい", WordKind::Adverb, Some("N3"), &["unintentionally", "just now"]),
    lex("そろそろ", "そろそろ", WordKind::Adverb, Some("N4"), &["soon", "it's about time"]),
    lex("まず", "まず", WordKind::Adverb, Some("N4"), &["first of all"]),
    lex("次に", "つぎに", WordKind::Adverb, Some("N4"), &["next", "secondly"]),
    lex("例えば", "たとえば", WordKind::Adverb, Some("N4"), &["for example"]),
    lex("たとえば", "たとえば", WordKind::Adverb, Some("N4"), &["for example"]),
    lex("なぜ", "なぜ", WordKind::Adverb, Some("N5"), &["why"]),
    lex("どうして", "どうして", WordKind::Adverb, Some("N5"), &["why", "how"]),
    lex("どう", "どう", WordKind::Adverb, Some("N5"), &["how", "in what way"]),
    lex("いつ", "いつ", WordKind::Adverb, Some("N5"), &["when"]),
    lex("いくら", "いくら", WordKind::Adverb, Some("N5"), &["how much"]),
    lex("いくつ", "いくつ", WordKind::Adverb, Some("N5"), &["how many", "how old"]),
    lex("いろいろ", "いろいろ", WordKind::NaAdj, Some("N5"), &["various"]),
    lex("なるべく", "なるべく", WordKind::Adverb, Some("N4"), &["as much as possible"]),

    // ==================== Numbers & Counters ====================
    lex("一つ", "ひとつ", WordKind::Number, Some("N5"), &["one (thing)"]),
    lex("二つ", "ふたつ", WordKind::Number, Some("N5"), &["two (things)"]),
    lex("三つ", "みっつ", WordKind::Number, Some("N5"), &["three (things)"]),
    lex("一人", "ひとり", WordKind::Number, Some("N5"), &["one person", "alone"]),
    lex("二人", "ふたり", WordKind::Number, Some("N5"), &["two people"]),
    lex("一", "いち", WordKind::Number, Some("N5"), &["one"]),
    lex("二", "に", WordKind::Number, Some("N5"), &["two"]),
    lex("三", "さん", WordKind::Number, Some("N5"), &["three"]),
    lex("四", "よん", WordKind::Number, Some("N5"), &["four"]),
    lex("五", "ご", WordKind::Number, Some("N5"), &["five"]),
    lex("六", "ろく", WordKind::Number, Some("N5"), &["six"]),
    lex("七", "なな", WordKind::Number, Some("N5"), &["seven"]),
    lex("八", "はち", WordKind::Number, Some("N5"), &["eight"]),
    lex("九", "きゅう", WordKind::Number, Some("N5"), &["nine"]),
    lex("十", "じゅう", WordKind::Number, Some("N5"), &["ten"]),
    lex("百", "ひゃく", WordKind::Number, Some("N5"), &["hundred"]),
    lex("千", "せん", WordKind::Number, Some("N5"), &["thousand"]),
    lex("万", "まん", WordKind::Number, Some("N5"), &["ten thousand"]),
    lex("円", "えん", WordKind::Noun, Some("N5"), &["yen"]),
];

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzes_reading_lab_default_sample() {
        let sample = "毎日日本語を勉強しているおかげで、少しずつ日本の新聞が読めるようになりました。雨にもかかわらず、公園へ行くつもりです。";
        let analysis = analyze_japanese_text(sample, &WkSubjectLookup::default());

        assert_eq!(analysis.analyzer_engine, ANALYZER_ENGINE_NAME);

        let find_tok = |surf: &str| {
            analysis
                .tokens
                .iter()
                .find(|t| t.surface == surf)
                .unwrap_or_else(|| {
                    panic!(
                        "missing token `{surf}`; tokens were: {:?}",
                        analysis
                            .tokens
                            .iter()
                            .map(|t| (&t.surface, &t.lemma, &t.reading))
                            .collect::<Vec<_>>()
                    )
                })
        };

        let mainichi = find_tok("毎日");
        assert_eq!(mainichi.reading, "まいにち");
        assert_eq!(mainichi.ruby, "[毎日|まいにち]");
        assert!(!mainichi.glosses.is_empty());

        let nihongo = find_tok("日本語");
        assert_eq!(nihongo.reading, "にほんご");
        assert_eq!(nihongo.ruby, "[日本語|にほんご]");
        assert!(!nihongo.glosses.is_empty());

        let benkyou = find_tok("勉強");
        assert_eq!(benkyou.reading, "べんきょう");
        assert_eq!(benkyou.ruby, "[勉強|べんきょう]");
        assert!(!benkyou.glosses.is_empty());

        let okagede = find_tok("おかげで");
        assert_eq!(okagede.reading, "おかげで");
        assert!(!okagede.glosses.is_empty());

        let sukoshizutsu = find_tok("少しずつ");
        assert_eq!(sukoshizutsu.reading, "すこしずつ");
        assert_eq!(sukoshizutsu.ruby, "[少|すこ]しずつ");
        assert!(!sukoshizutsu.glosses.is_empty());

        let shinbun = find_tok("新聞");
        assert_eq!(shinbun.reading, "しんぶん");
        assert_eq!(shinbun.ruby, "[新聞|しんぶん]");
        assert!(!shinbun.glosses.is_empty());

        let yomeru = find_tok("読める");
        assert_eq!(yomeru.lemma, "読む");
        assert_eq!(yomeru.reading, "よめる");
        assert_eq!(yomeru.ruby, "[読|よ]める");
        assert_eq!(yomeru.pos, "verb");
        assert!(!yomeru.glosses.is_empty());

        let ame = find_tok("雨");
        assert_eq!(ame.reading, "あめ");
        assert_eq!(ame.ruby, "[雨|あめ]");

        let nimokakawarazu = find_tok("にもかかわらず");
        assert_eq!(nimokakawarazu.reading, "にもかかわらず");
        assert!(!nimokakawarazu.glosses.is_empty());

        let kouen = find_tok("公園");
        assert_eq!(kouen.reading, "こうえん");
        assert_eq!(kouen.ruby, "[公園|こうえん]");

        let iku = find_tok("行く");
        assert_eq!(iku.lemma, "行く");
        assert_eq!(iku.reading, "いく");
        assert_eq!(iku.ruby, "[行|い]く");

        let tsumori = find_tok("つもり");
        assert_eq!(tsumori.reading, "つもり");
        assert!(!tsumori.glosses.is_empty());

        assert_eq!(
            analysis.annotated_ruby,
            "[毎日|まいにち][日本語|にほんご]を[勉強|べんきょう]しているおかげで、[少|すこ]しずつ[日本|にほん]の[新聞|しんぶん]が[読|よ]めるようになりました。[雨|あめ]にもかかわらず、[公園|こうえん]へ[行|い]くつもりです。"
        );
    }

    #[test]
    fn okurigana_aware_ruby_splitting() {
        assert_eq!(format_ruby("食べる", "たべる"), "[食|た]べる");
        assert_eq!(format_ruby("美しい", "うつくしい"), "[美|うつく]しい");
        assert_eq!(format_ruby("新聞", "しんぶん"), "[新聞|しんぶん]");
        assert_eq!(format_ruby("少しずつ", "すこしずつ"), "[少|すこ]しずつ");
        assert_eq!(format_ruby("お茶", "おちゃ"), "お[茶|ちゃ]");
        assert_eq!(format_ruby("乗り換え", "のりかえ"), "[乗|の]り[換|か]え");
        assert_eq!(format_ruby("ひらがな", "ひらがな"), "ひらがな");
    }

    #[test]
    fn deconjugation_handles_common_inflections() {
        let has_lemma = |surf: &str, expected: &str| {
            let cands = deconjugate_surface(surf);
            assert!(
                cands.iter().any(|(l, _)| l == expected),
                "expected `{surf}` to deconjugate to `{expected}`, got {cands:?}"
            );
        };

        has_lemma("食べなかった", "食べる");
        has_lemma("読める", "読む");
        has_lemma("読めなかった", "読む");
        has_lemma("高かった", "高い");
        has_lemma("行きました", "行く");
        has_lemma("勉強している", "勉強する");
        has_lemma("買わなければ", "買う");
        has_lemma("来ない", "来る");
        has_lemma("行われる", "行う");
        has_lemma("行われた", "行う");
        has_lemma("静かだった", "静か");
        has_lemma("良くない", "良い");
    }

    #[test]
    fn wanikani_enrichment_sets_level_and_known_flags() {
        let mut wk = WkSubjectLookup::default();
        wk.vocab_levels.insert("新聞".to_string(), 10);
        wk.kanji_levels.insert("読".to_string(), 10);
        wk.kanji_levels.insert("新".to_string(), 9);
        wk.kanji_levels.insert("聞".to_string(), 10);
        wk.known_kanji.insert("新".to_string());
        wk.known_kanji.insert("聞".to_string());

        let analysis = analyze_japanese_text("新聞が読める。", &wk);
        let shinbun = analysis.tokens.iter().find(|t| t.surface == "新聞").unwrap();
        assert_eq!(shinbun.wk_level, Some(10));
        assert!(shinbun.wk_known);

        let yomeru = analysis.tokens.iter().find(|t| t.surface == "読める").unwrap();
        assert_eq!(yomeru.wk_level, Some(10));
        assert!(!yomeru.wk_known);

        let ga = analysis.tokens.iter().find(|t| t.surface == "が").unwrap();
        assert_eq!(ga.wk_level, None);
        assert!(!ga.wk_known);
    }

    #[test]
    fn grammar_pattern_matching_handles_inflections_and_avoids_false_positives() {
        let wk = WkSubjectLookup::default();

        let check_match = |text: &str, pattern: &str| {
            let analysis = analyze_japanese_text(text, &wk);
            let res = match_pattern_against_analysis(pattern, text, &analysis);
            assert!(
                res.is_some(),
                "expected pattern `{pattern}` to match `{text}` (tokens: {:?})",
                analysis
                    .tokens
                    .iter()
                    .map(|t| (&t.surface, &t.lemma))
                    .collect::<Vec<_>>()
            );
        };

        let check_no_match = |text: &str, pattern: &str| {
            let analysis = analyze_japanese_text(text, &wk);
            let res = match_pattern_against_analysis(pattern, text, &analysis);
            assert!(
                res.is_none(),
                "expected pattern `{pattern}` NOT to match `{text}`, but matched {res:?}"
            );
        };

        check_match("毎日ご飯を食べている。", "ている");
        check_match("昨日勉強していました。", "ている");
        check_match("日本の新聞が読めるようになりました。", "ようになる");
        check_match("勉強しているおかげで合格しました。", "おかげで");
        check_match("雨にもかかわらず、公園へ行く。", "にもかかわらず");
        check_match("明日早く行かなければなりません。", "なければならない");
        check_match("新しい傘を買わなければなりませんでした。", "なければならない");
        check_match("公園へ行くつもりです。", "つもり");
        check_match("公園へ行くつもりです。", "つもりだ");
        check_match("生活が大変なわけではありません。", "わけではない");
        check_match("遅れるわけにはいきません。", "わけにはいかない");
        check_match("日本へ行きたいです。", "たい");

        // False-positive avoidance
        check_no_match("台風が近づいている。", "たい");
        check_no_match("大切な約束です。", "たい");
        check_no_match("頭が痛いです。", "たい");
        check_no_match("昨日の荷物です。", "のに");
        check_no_match("まだ分からない漢字があります。", "から");
    }

    #[test]
    fn strip_and_auto_annotate_ruby_roundtrip() {
        let ruby = "[毎日|まいにち][日本語|にほんご]を[勉強|べんきょう]する。";
        let plain = strip_ruby_text(ruby);
        assert_eq!(plain, "毎日日本語を勉強する。");
        assert_eq!(auto_annotate_ruby(&plain), ruby);
    }

    #[test]
    fn analyzes_all_seed_passages_cleanly() {
        let passages = [
            "毎日日本語を勉強しているおかげで、少しずつ日本の新聞が読めるようになりました。まだ分からない漢字もありますが、毎日読んでみることにしています。来年、日本で働くために、今も頑張っています。",
            "昨日は雨が降ったのに、友達と公園へ行きました。傘を忘れてしまいましたから、駅で新しい傘を買わなければなりませんでした。でも、一緒に温かいお茶を飲みながら話して、とても楽しかったです。",
            "先月日本に来たばかりなので、まだ電車の乗り換えに慣れていません。国によって規則や挨拶の仕方が違います。生活が大変なわけではありませんが、早く日本語が上手になるようにしています。",
            "台風が近づいているにもかかわらず、会議は予定通り行われるはずです。大切な約束なので、遅れるわけにはいきません。大雨のせいで交通が乱れるに違いないので、早めに出発しておくつもりです。",
        ];

        let analyzer = EmbeddedJapaneseAnalyzer;
        let wk = WkSubjectLookup::default();
        for p in passages {
            let analysis = analyzer.analyze(p, &wk);
            let stripped = strip_ruby_text(&analysis.annotated_ruby);
            assert_eq!(stripped, p, "ruby roundtrip must preserve exact surface text");
            // Ensure no kanji-bearing word in our seed passages was left without furigana
            assert!(
                !analysis.tokens.iter().any(|t| t.surface.chars().any(is_kanji) && t.reading.is_empty()),
                "all kanji tokens in seed passage should have readings: {:?}",
                analysis
                    .tokens
                    .iter()
                    .filter(|t| t.surface.chars().any(is_kanji) && t.reading.is_empty())
                    .map(|t| &t.surface)
                    .collect::<Vec<_>>()
            );
        }
    }
}
