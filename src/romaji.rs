use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use lindera::tokenizer::Tokenizer;
use lindera::LinderaResult;

/// Check if a string contains Japanese characters (Hiragana, Katakana, or CJK).
pub fn contains_japanese(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c as u32,
            0x3040..=0x309F |  // Hiragana
            0x30A0..=0x30FF |  // Katakana
            0x4E00..=0x9FFF    // CJK Unified Ideographs
        )
    })
}

/// Build a lindera Tokenizer with the embedded IPADIC dictionary.
/// This is expensive to create — call once and reuse.
pub fn build_tokenizer() -> LinderaResult<Tokenizer> {
    let dictionary = load_dictionary("embedded://ipadic")?;
    let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
    Ok(Tokenizer::new(segmenter))
}

/// Convert Japanese text to (kana_reading, romaji) using lindera + wana_kana.
///
/// Returns `(kana_string, romaji_string)` where:
/// - `kana_string` is the reading in Katakana from lindera IPADIC
/// - `romaji_string` is the romanized form via wana_kana
pub fn to_kana_and_romaji(tokenizer: &Tokenizer, text: &str) -> (String, String) {
    let mut kana_parts: Vec<String> = Vec::new();

    // Tokenize and extract readings
    match tokenizer.tokenize(text) {
        Ok(tokens) => {
            for token in &tokens {
                let surface = token.surface.as_ref();
                // IPADIC token details are CSV fields:
                // [0] pos, [1] subpos1, [2] subpos2, [3] subpos3,
                // [4] conjugation_type, [5] conjugation_form,
                // [6] base_form, [7] reading, [8] pronunciation
                if let Some(ref details) = token.details {
                    if details.len() > 7 {
                        let reading = details[7].as_ref();
                        if reading != "*" && !reading.is_empty() {
                            kana_parts.push(reading.to_string());
                        } else {
                            kana_parts.push(surface.to_string());
                        }
                    } else {
                        kana_parts.push(surface.to_string());
                    }
                } else {
                    kana_parts.push(surface.to_string());
                }
            }
        }
        Err(e) => {
            log::warn!("Lindera tokenization failed: {}", e);
            kana_parts.push(text.to_string());
        }
    }

    let kana_text = kana_parts.join("");
    
    // wana_kana v4 uses the ConvertJapanese trait
    use wana_kana::ConvertJapanese;
    let romaji_text = kana_text.to_romaji();

    (kana_text, romaji_text)
}

/// Process lyrics lines: for each line, if it contains Japanese,
/// produce kana and romaji versions.
///
/// Returns parallel vectors of (kana, romaji) for each input line.
/// Non-Japanese lines get `(None, None)`.
pub fn process_lyrics_lines(
    tokenizer: &Tokenizer,
    lines: &[&str],
) -> Vec<(Option<String>, Option<String>)> {
    lines
        .iter()
        .map(|line| {
            if contains_japanese(line) {
                let (kana, romaji) = to_kana_and_romaji(tokenizer, line);
                (Some(kana), Some(romaji))
            } else {
                (None, None)
            }
        })
        .collect()
}
