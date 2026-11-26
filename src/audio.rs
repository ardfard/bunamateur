use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_AUDIO_CACHE_DIR: &str = "./data/audio";
pub const DEFAULT_VOICEVOX_URL: &str = "http://127.0.0.1:50021";
pub const DEFAULT_VOICEVOX_SPEAKER_ID: u32 = 3;
pub const DEFAULT_AZURE_SPEECH_VOICE: &str = "ja-JP-NanamiNeural";

const CACHE_EXTENSIONS: [&str; 3] = ["opus", "mp3", "wav"];
const AZURE_OUTPUT_FORMAT: &str = "audio-24khz-96kbitrate-mono-mp3";
const TTS_REQUEST_TIMEOUT_SECS: u64 = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioProviderKind {
    #[default]
    None,
    Voicevox,
    Azure,
}

impl AudioProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Voicevox => "voicevox",
            Self::Azure => "azure",
        }
    }
}

impl fmt::Display for AudioProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AudioProviderKind {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "none" | "disabled" | "off" => Ok(Self::None),
            "voicevox" => Ok(Self::Voicevox),
            "azure" | "azure_speech" | "azure-speech" => Ok(Self::Azure),
            other => bail!(
                "Unsupported AUDIO_PROVIDER '{other}'. Expected one of: none, voicevox, azure"
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub cache_dir: PathBuf,
    pub cdn_base_url: Option<String>,
    pub provider: AudioProviderKind,
    pub voicevox_url: String,
    pub voicevox_speaker: u32,
    pub azure_speech_key: Option<String>,
    pub azure_speech_region: Option<String>,
    pub azure_voice: String,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from(DEFAULT_AUDIO_CACHE_DIR),
            cdn_base_url: None,
            provider: AudioProviderKind::None,
            voicevox_url: DEFAULT_VOICEVOX_URL.to_string(),
            voicevox_speaker: DEFAULT_VOICEVOX_SPEAKER_ID,
            azure_speech_key: None,
            azure_speech_region: None,
            azure_voice: DEFAULT_AZURE_SPEECH_VOICE.to_string(),
        }
    }
}

impl AudioConfig {
    /// Loads audio cache, CDN, and TTS provider settings from environment variables.
    ///
    /// - `AUDIO_CACHE_DIR`: Local directory for pre-generated or cached audio files (default `./data/audio`).
    /// - `AUDIO_CDN_BASE_URL`: Optional CDN base URL for audio redirects (trailing `/` trimmed).
    /// - `AUDIO_PROVIDER`: `none` | `voicevox` | `azure` (default `none`).
    /// - `VOICEVOX_URL`: VOICEVOX engine base URL (default `http://127.0.0.1:50021`).
    /// - `VOICEVOX_SPEAKER_ID`: VOICEVOX speaker/style ID (default `3`).
    /// - `AZURE_SPEECH_KEY`: Azure Cognitive Services Speech subscription key.
    /// - `AZURE_SPEECH_REGION`: Azure Cognitive Services Speech region (e.g. `japaneast`).
    /// - `AZURE_SPEECH_VOICE`: Azure Neural voice name (default `ja-JP-NanamiNeural`).
    pub fn from_env() -> Self {
        let cache_dir = env_non_empty("AUDIO_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_AUDIO_CACHE_DIR));

        let cdn_base_url = env_non_empty("AUDIO_CDN_BASE_URL")
            .map(|raw| raw.trim_end_matches('/').to_string())
            .filter(|v| !v.is_empty());

        let provider = match env_non_empty("AUDIO_PROVIDER") {
            Some(raw) => match raw.parse::<AudioProviderKind>() {
                Ok(kind) => kind,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "Invalid AUDIO_PROVIDER value; defaulting to 'none'"
                    );
                    AudioProviderKind::None
                }
            },
            None => AudioProviderKind::None,
        };

        let voicevox_url = env_non_empty("VOICEVOX_URL")
            .map(|raw| raw.trim_end_matches('/').to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_VOICEVOX_URL.to_string());

        let voicevox_speaker = match env_non_empty("VOICEVOX_SPEAKER_ID") {
            Some(raw) => match raw.parse::<u32>() {
                Ok(id) => id,
                Err(_) => {
                    tracing::warn!(
                        value = %raw,
                        default = DEFAULT_VOICEVOX_SPEAKER_ID,
                        "Invalid VOICEVOX_SPEAKER_ID; using default"
                    );
                    DEFAULT_VOICEVOX_SPEAKER_ID
                }
            },
            None => DEFAULT_VOICEVOX_SPEAKER_ID,
        };

        let azure_speech_key = env_non_empty("AZURE_SPEECH_KEY");
        let azure_speech_region = env_non_empty("AZURE_SPEECH_REGION");
        let azure_voice = env_non_empty("AZURE_SPEECH_VOICE")
            .unwrap_or_else(|| DEFAULT_AZURE_SPEECH_VOICE.to_string());

        Self {
            cache_dir,
            cdn_base_url,
            provider,
            voicevox_url,
            voicevox_speaker,
            azure_speech_key,
            azure_speech_region,
            azure_voice,
        }
    }

    pub fn provider_name(&self) -> &'static str {
        self.provider.as_str()
    }
}

/// Strips inline ruby markup (`[漢字|かんじ]` -> `漢字`) while leaving plain text
/// and non-ruby bracket expressions intact.
pub fn strip_ruby_for_audio(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while cursor < input.len() {
        let rest = &input[cursor..];
        let Some(open_rel) = rest.find('[') else {
            output.push_str(rest);
            break;
        };

        output.push_str(&rest[..open_rel]);
        let after_open = cursor + open_rel + 1;
        let tail = &input[after_open..];

        if let Some(close_rel) = tail.find(']') {
            let inner = &tail[..close_rel];
            if !inner.contains('[')
                && let Some((base, _reading)) = inner.split_once('|')
                && !base.is_empty()
            {
                output.push_str(base);
                cursor = after_open + close_rel + 1;
                continue;
            }
        }

        output.push('[');
        cursor = after_open;
    }

    output
}

/// Computes the deterministic SHA-256 hex digest of trimmed, ruby-stripped Japanese text.
pub fn compute_audio_hash(plain_text: &str) -> String {
    let stripped = strip_ruby_for_audio(plain_text);
    let normalized = stripped.trim();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

/// Escapes XML special characters so arbitrary text is safe inside Azure SSML payloads.
pub fn escape_xml(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Builds a safe ja-JP SSML document for Azure Neural Speech synthesis.
pub fn build_azure_ssml(voice: &str, plain_text: &str) -> String {
    let escaped_voice = escape_xml(voice);
    let escaped_text = escape_xml(plain_text);
    format!(
        "<speak version='1.0' xml:lang='ja-JP'><voice xml:lang='ja-JP' name='{escaped_voice}'>{escaped_text}</voice></speak>"
    )
}

/// Returns the HTTP MIME type associated with a cached audio file extension.
pub fn mime_type_for_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "opus" => "audio/opus",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Clone)]
pub struct SynthesizedAudio {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub extension: String,
    pub provider: String,
    pub voice: String,
}

#[derive(Debug, Clone)]
pub enum AudioResolution {
    Redirect(String),
    Bytes {
        bytes: Vec<u8>,
        mime_type: String,
        etag: String,
        source: &'static str,
    },
    NotAvailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioStatusSummary {
    pub provider: &'static str,
    pub cdn_configured: bool,
    pub cache_dir: String,
    pub cached_files: usize,
}

#[derive(Clone)]
pub struct AudioService {
    config: AudioConfig,
    http: Client,
}

impl AudioService {
    pub fn new(config: AudioConfig) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(TTS_REQUEST_TIMEOUT_SECS))
            .user_agent("BunAmateur/0.1")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { config, http }
    }

    pub fn config(&self) -> &AudioConfig {
        &self.config
    }

    /// Resolves audio for a grammar sentence or reading passage using layered fallback:
    /// 1. Explicit `http://` or `https://` URL -> `AudioResolution::Redirect`.
    /// 2. Local disk cache (`sentence_{id}.{opus,mp3,wav}` then `{text_hash}.{opus,mp3,wav}`).
    /// 3. Configured CDN base URL (`{cdn_base_url}/{text_hash}.mp3`) -> `AudioResolution::Redirect`.
    /// 4. On-demand synthesis via configured TTS provider (VOICEVOX or Azure) with atomic disk caching.
    /// 5. `AudioResolution::NotAvailable` so the frontend can gracefully fall back to Web Speech API.
    pub async fn resolve_sentence_audio(
        &self,
        sentence_id: i64,
        explicit_audio_url: &str,
        plain_text: &str,
    ) -> Result<AudioResolution> {
        let trimmed_url = explicit_audio_url.trim();
        if trimmed_url.starts_with("http://") || trimmed_url.starts_with("https://") {
            return Ok(AudioResolution::Redirect(trimmed_url.to_string()));
        }

        let stripped = strip_ruby_for_audio(plain_text);
        let normalized = stripped.trim();
        let text_hash = compute_audio_hash(normalized);

        if let Some(cached) = self.lookup_local_cache(sentence_id, &text_hash, !normalized.is_empty()).await {
            return Ok(cached);
        }

        if normalized.is_empty() {
            return Ok(AudioResolution::NotAvailable);
        }

        if let Some(base) = &self.config.cdn_base_url
            && !base.is_empty()
        {
            return Ok(AudioResolution::Redirect(format!("{base}/{text_hash}.mp3")));
        }

        if self.config.provider != AudioProviderKind::None {
            let synthesized = self.synthesize(normalized).await?;
            let filename = format!("{text_hash}.{}", synthesized.extension);
            if let Err(err) = write_cache_atomic(&self.config.cache_dir, &filename, &synthesized.bytes).await {
                tracing::warn!(
                    cache_dir = %self.config.cache_dir.display(),
                    filename = %filename,
                    error = %err,
                    "Synthesized audio succeeded but caching to disk failed"
                );
            }

            let etag = compute_weak_etag(&synthesized.bytes);
            return Ok(AudioResolution::Bytes {
                bytes: synthesized.bytes,
                mime_type: synthesized.mime_type,
                etag,
                source: self.config.provider_name(),
            });
        }

        Ok(AudioResolution::NotAvailable)
    }

    /// Synthesizes Japanese audio on-demand via the configured TTS provider.
    pub async fn synthesize(&self, plain_text: &str) -> Result<SynthesizedAudio> {
        let stripped = strip_ruby_for_audio(plain_text);
        let text = stripped.trim();
        if text.is_empty() {
            bail!("Cannot synthesize audio for empty text");
        }

        match self.config.provider {
            AudioProviderKind::None => bail!("Audio TTS provider is not configured"),
            AudioProviderKind::Voicevox => self.synthesize_voicevox(text).await,
            AudioProviderKind::Azure => self.synthesize_azure(text).await,
        }
    }

    /// Returns a lightweight diagnostic summary of the local audio cache and provider state.
    pub fn cache_status(&self) -> AudioStatusSummary {
        let cached_files = count_cached_audio_files(&self.config.cache_dir);
        AudioStatusSummary {
            provider: self.config.provider_name(),
            cdn_configured: self
                .config
                .cdn_base_url
                .as_deref()
                .is_some_and(|v| !v.is_empty()),
            cache_dir: self.config.cache_dir.display().to_string(),
            cached_files,
        }
    }

    async fn lookup_local_cache(
        &self,
        sentence_id: i64,
        text_hash: &str,
        has_text: bool,
    ) -> Option<AudioResolution> {
        let mut prefixes = Vec::with_capacity(2);
        prefixes.push(format!("sentence_{sentence_id}"));
        if has_text {
            prefixes.push(text_hash.to_string());
        }

        for prefix in prefixes {
            for ext in CACHE_EXTENSIONS {
                let candidate = self.config.cache_dir.join(format!("{prefix}.{ext}"));
                match tokio::fs::read(&candidate).await {
                    Ok(bytes) if !bytes.is_empty() => {
                        let etag = compute_weak_etag(&bytes);
                        return Some(AudioResolution::Bytes {
                            bytes,
                            mime_type: mime_type_for_extension(ext).to_string(),
                            etag,
                            source: "cache",
                        });
                    }
                    Ok(_) => continue,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => {
                        tracing::warn!(
                            path = %candidate.display(),
                            error = %err,
                            "Failed reading cached audio file; continuing fallback"
                        );
                    }
                }
            }
        }

        None
    }

    async fn synthesize_voicevox(&self, text: &str) -> Result<SynthesizedAudio> {
        let base_url = self.config.voicevox_url.trim_end_matches('/');
        let speaker_str = self.config.voicevox_speaker.to_string();

        let query_resp = self
            .http
            .post(format!("{base_url}/audio_query"))
            .query(&[("text", text), ("speaker", speaker_str.as_str())])
            .send()
            .await
            .context("VOICEVOX /audio_query request failed")?;

        if !query_resp.status().is_success() {
            let status = query_resp.status();
            let body = query_resp.text().await.unwrap_or_default();
            bail!("VOICEVOX /audio_query returned {status}: {body}");
        }

        let query_json = query_resp
            .bytes()
            .await
            .context("failed to read VOICEVOX /audio_query response body")?;

        let synth_resp = self
            .http
            .post(format!("{base_url}/synthesis"))
            .query(&[("speaker", speaker_str.as_str())])
            .header("Content-Type", "application/json")
            .body(query_json)
            .send()
            .await
            .context("VOICEVOX /synthesis request failed")?;

        if !synth_resp.status().is_success() {
            let status = synth_resp.status();
            let body = synth_resp.text().await.unwrap_or_default();
            bail!("VOICEVOX /synthesis returned {status}: {body}");
        }

        let wav_bytes = synth_resp
            .bytes()
            .await
            .context("failed to read VOICEVOX /synthesis audio bytes")?
            .to_vec();

        if wav_bytes.is_empty() {
            bail!("VOICEVOX /synthesis returned empty audio payload");
        }

        Ok(SynthesizedAudio {
            bytes: wav_bytes,
            mime_type: "audio/wav".to_string(),
            extension: "wav".to_string(),
            provider: "voicevox".to_string(),
            voice: format!("speaker-{}", self.config.voicevox_speaker),
        })
    }

    async fn synthesize_azure(&self, text: &str) -> Result<SynthesizedAudio> {
        let key = self
            .config
            .azure_speech_key
            .as_deref()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow!("AZURE_SPEECH_KEY is required when AUDIO_PROVIDER=azure"))?;
        let region = self
            .config
            .azure_speech_region
            .as_deref()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow!("AZURE_SPEECH_REGION is required when AUDIO_PROVIDER=azure"))?;

        let endpoint = format!("https://{region}.tts.speech.microsoft.com/cognitiveservices/v1");
        let ssml = build_azure_ssml(&self.config.azure_voice, text);

        let resp = self
            .http
            .post(&endpoint)
            .header("Ocp-Apim-Subscription-Key", key)
            .header("Content-Type", "application/ssml+xml")
            .header("X-Microsoft-OutputFormat", AZURE_OUTPUT_FORMAT)
            .header("User-Agent", "BunAmateur")
            .body(ssml)
            .send()
            .await
            .context("Azure Neural Speech synthesis request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Azure Neural Speech returned {status}: {body}");
        }

        let mp3_bytes = resp
            .bytes()
            .await
            .context("failed to read Azure Neural Speech audio bytes")?
            .to_vec();

        if mp3_bytes.is_empty() {
            bail!("Azure Neural Speech returned empty audio payload");
        }

        Ok(SynthesizedAudio {
            bytes: mp3_bytes,
            mime_type: "audio/mpeg".to_string(),
            extension: "mp3".to_string(),
            provider: "azure".to_string(),
            voice: self.config.azure_voice.clone(),
        })
    }
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn compute_weak_etag(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hex::encode(hasher.finalize());
    format!("W/\"{}\"", &digest[..16])
}

fn count_cached_audio_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };

    entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "opus" | "mp3" | "wav" | "ogg" | "m4a"
                    )
                })
        })
        .count()
}

async fn write_cache_atomic(dir: &Path, filename: &str, bytes: &[u8]) -> Result<PathBuf> {
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("failed to create audio cache directory {}", dir.display()))?;

    let final_path = dir.join(filename);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!(".{filename}.{}.{nanos}.tmp", std::process::id());
    let tmp_path = dir.join(tmp_name);

    if let Err(err) = tokio::fs::write(&tmp_path, bytes).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(err).with_context(|| {
            format!("failed to write temporary audio file {}", tmp_path.display())
        });
    }

    if let Err(err) = tokio::fs::rename(&tmp_path, &final_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(err).with_context(|| {
            format!(
                "failed to promote temporary audio file {} to {}",
                tmp_path.display(),
                final_path.display()
            )
        });
    }

    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn create_temp_cache_dir(label: &str) -> PathBuf {
        let seq = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "bunamateur_audio_test_{label}_{}_{seq}_{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp audio test dir");
        dir
    }

    #[test]
    fn strip_ruby_and_hash_are_deterministic() {
        let ruby = "  [今|いま]、[図書館|としょかん]で[日本語|にほんご]を[勉強|べんきょう]しています。 ";
        let plain = "今、図書館で日本語を勉強しています。";

        assert_eq!(strip_ruby_for_audio(ruby).trim(), plain);
        assert_eq!(compute_audio_hash(ruby), compute_audio_hash(plain));
        assert_eq!(compute_audio_hash(plain).len(), 64);

        // Non-ruby brackets and malformed tags remain intact
        assert_eq!(
            strip_ruby_for_audio("[注] [未完|みかん and [漢字|かんじ]"),
            "[注] [未完|みかん and 漢字"
        );
    }

    #[test]
    fn escape_xml_and_ssml_builder_protect_special_chars() {
        let raw = "A & B <「日本語」> \"test\" 'single'";
        assert_eq!(
            escape_xml(raw),
            "A &amp; B &lt;「日本語」&gt; &quot;test&quot; &apos;single&apos;"
        );

        let ssml = build_azure_ssml("ja-JP-NanamiNeural", "1 < 2 & 3 > 0");
        assert_eq!(
            ssml,
            "<speak version='1.0' xml:lang='ja-JP'><voice xml:lang='ja-JP' name='ja-JP-NanamiNeural'>1 &lt; 2 &amp; 3 &gt; 0</voice></speak>"
        );
    }

    #[test]
    fn provider_kind_parsing_and_display() {
        assert_eq!("none".parse::<AudioProviderKind>().unwrap(), AudioProviderKind::None);
        assert_eq!("voicevox".parse::<AudioProviderKind>().unwrap(), AudioProviderKind::Voicevox);
        assert_eq!("AZURE".parse::<AudioProviderKind>().unwrap(), AudioProviderKind::Azure);
        assert!("unknown".parse::<AudioProviderKind>().is_err());
        assert_eq!(AudioProviderKind::Voicevox.to_string(), "voicevox");
    }

    #[tokio::test]
    async fn local_cache_resolves_opus_mp3_wav_with_mime_and_etag() {
        let dir = create_temp_cache_dir("cache_lookup");
        let text = "[今日|きょう]はいい[天気|てんき]です。";
        let hash = compute_audio_hash(text);

        let config = AudioConfig {
            cache_dir: dir.clone(),
            ..AudioConfig::default()
        };
        let service = AudioService::new(config);

        // 1. Hash-based .wav
        let wav_path = dir.join(format!("{hash}.wav"));
        std::fs::write(&wav_path, b"RIFF_FAKE_WAV").unwrap();
        match service.resolve_sentence_audio(10, "", text).await.unwrap() {
            AudioResolution::Bytes {
                bytes,
                mime_type,
                etag,
                source,
            } => {
                assert_eq!(bytes, b"RIFF_FAKE_WAV");
                assert_eq!(mime_type, "audio/wav");
                assert!(etag.starts_with("W/\"") && etag.ends_with('"'));
                assert_eq!(source, "cache");
            }
            other => panic!("expected wav cache bytes, got {other:?}"),
        }

        // 2. Hash-based .mp3 takes priority over .wav
        let mp3_path = dir.join(format!("{hash}.mp3"));
        std::fs::write(&mp3_path, b"ID3_FAKE_MP3").unwrap();
        match service.resolve_sentence_audio(10, "", text).await.unwrap() {
            AudioResolution::Bytes {
                bytes,
                mime_type,
                etag,
                source,
            } => {
                assert_eq!(bytes, b"ID3_FAKE_MP3");
                assert_eq!(mime_type, "audio/mpeg");
                assert_eq!(etag, compute_weak_etag(b"ID3_FAKE_MP3"));
                assert_eq!(source, "cache");
            }
            other => panic!("expected mp3 cache bytes, got {other:?}"),
        }

        // 3. sentence_{id}.opus takes highest local priority over hash-based files
        let opus_path = dir.join("sentence_10.opus");
        std::fs::write(&opus_path, b"OggS_FAKE_OPUS").unwrap();
        match service.resolve_sentence_audio(10, "", text).await.unwrap() {
            AudioResolution::Bytes {
                bytes,
                mime_type,
                etag,
                source,
            } => {
                assert_eq!(bytes, b"OggS_FAKE_OPUS");
                assert_eq!(mime_type, "audio/opus");
                assert_eq!(etag, compute_weak_etag(b"OggS_FAKE_OPUS"));
                assert_eq!(source, "cache");
            }
            other => panic!("expected opus cache bytes, got {other:?}"),
        }

        let summary = service.cache_status();
        assert_eq!(summary.cached_files, 3);
        assert_eq!(summary.provider, "none");
        assert!(!summary.cdn_configured);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn explicit_url_and_cdn_redirects() {
        let dir = create_temp_cache_dir("redirects");
        let text = "日本語のテスト";
        let hash = compute_audio_hash(text);

        let config = AudioConfig {
            cache_dir: dir.clone(),
            cdn_base_url: Some("https://cdn.example.com/audio".to_string()),
            ..AudioConfig::default()
        };
        let service = AudioService::new(config);

        // Explicit HTTPS URL always redirects first
        match service
            .resolve_sentence_audio(7, "https://storage.example.com/custom.mp3", text)
            .await
            .unwrap()
        {
            AudioResolution::Redirect(url) => {
                assert_eq!(url, "https://storage.example.com/custom.mp3");
            }
            other => panic!("expected explicit redirect, got {other:?}"),
        }

        // Without local cache file, falls back to CDN base URL
        match service.resolve_sentence_audio(7, "", text).await.unwrap() {
            AudioResolution::Redirect(url) => {
                assert_eq!(url, format!("https://cdn.example.com/audio/{hash}.mp3"));
            }
            other => panic!("expected CDN redirect, got {other:?}"),
        }

        assert!(service.cache_status().cdn_configured);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn returns_not_available_when_unconfigured() {
        let dir = create_temp_cache_dir("unconfigured");
        let config = AudioConfig {
            cache_dir: dir.clone(),
            cdn_base_url: None,
            provider: AudioProviderKind::None,
            ..AudioConfig::default()
        };
        let service = AudioService::new(config);

        let res = service
            .resolve_sentence_audio(99, "", "未設定のテスト文です。")
            .await
            .unwrap();
        assert!(matches!(res, AudioResolution::NotAvailable));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
