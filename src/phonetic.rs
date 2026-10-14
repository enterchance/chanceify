//! Lyrics respelled in Latin letters, so a song in a language you do not
//! read can still be sung along to. Russian and Korean are transliterated
//! letter by letter, Chinese is written in pinyin, and German and Turkish
//! are respelled the way an English reader would say them.
//!
//! These are plain approximations, not dictionary pronunciations.

use pinyin::ToPinyin;

/// The line in Latin letters, or `None` when it needs no help (it is already
/// Latin script with nothing to respell, or a language that is not covered).
pub fn respell(line: &str) -> Option<String> {
    let mut has = [false; 3]; // cyrillic, hangul, han
    for c in line.chars() {
        match c {
            '\u{0400}'..='\u{04FF}' => has[0] = true,
            '\u{AC00}'..='\u{D7A3}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' => has[1] = true,
            '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' => has[2] = true,
            _ => {}
        }
    }
    let out = if has[0] {
        cyrillic(line)
    } else if has[1] {
        hangul(line)
    } else if has[2] {
        han(line)
    } else if line.chars().any(|c| "äöüßÄÖÜ".contains(c)) || looks_german(line) {
        german(line)
    } else if line.chars().any(|c| "çşğıİöüÇŞĞ".contains(c)) {
        turkish(line)
    } else {
        return None;
    };
    (!out.trim().is_empty() && out != line).then_some(out)
}

fn looks_german(line: &str) -> bool {
    const WORDS: [&str; 14] = [
        "ich", "und", "nicht", "das", "ist", "du", "ein", "mit", "wir", "sie", "mein", "dich", "auch", "nur",
    ];
    let lower = line.to_lowercase();
    lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|word| WORDS.contains(word))
        .count()
        >= 2
}

fn cyrillic(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        let lower = c.to_lowercase().next().unwrap_or(c);
        let upper = c.is_uppercase();
        let s = match lower {
            'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' => "ye",
            'ё' => "yo", 'ж' => "zh", 'з' => "z", 'и' => "ee", 'й' => "y", 'к' => "k",
            'л' => "l", 'м' => "m", 'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r",
            'с' => "s", 'т' => "t", 'у' => "oo", 'ф' => "f", 'х' => "kh", 'ц' => "ts",
            'ч' => "ch", 'ш' => "sh", 'щ' => "shch", 'ъ' => "", 'ы' => "i", 'ь' => "'",
            'э' => "e", 'ю' => "yoo", 'я' => "ya", 'і' => "ee", 'ї' => "yee", 'є' => "ye",
            'ґ' => "g",
            _ => {
                out.push(c);
                continue;
            }
        };
        if upper {
            let mut chars = s.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push_str(s);
        }
    }
    out
}

fn han(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        match c.to_pinyin() {
            Some(p) => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(p.with_tone());
                out.push(' ');
            }
            None => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hangul(line: &str) -> String {
    const INITIAL: [&str; 19] = [
        "g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p", "h",
    ];
    const VOWEL: [&str; 21] = [
        "a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we", "wi",
        "yu", "eu", "ui", "i",
    ];
    const FINAL: [&str; 28] = [
        "", "k", "k", "k", "n", "n", "n", "t", "l", "k", "m", "l", "l", "l", "p", "l", "m", "p", "p", "t", "t",
        "ng", "t", "t", "k", "t", "p", "t",
    ];
    let mut out = String::new();
    for c in line.chars() {
        let code = c as u32;
        if (0xAC00..=0xD7A3).contains(&code) {
            let index = code - 0xAC00;
            let initial = (index / (21 * 28)) as usize;
            let vowel = ((index % (21 * 28)) / 28) as usize;
            let last = (index % 28) as usize;
            out.push_str(INITIAL[initial]);
            out.push_str(VOWEL[vowel]);
            out.push_str(FINAL[last]);
        } else {
            out.push(c);
        }
    }
    out
}

fn german(line: &str) -> String {
    let chars: Vec<char> = line.to_lowercase().chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let at = |i: usize, pattern: &str| {
        pattern.chars().enumerate().all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    while i < chars.len() {
        let step = if at(i, "tsch") { out.push_str("ch"); 4 }
        else if at(i, "sch") { out.push_str("sh"); 3 }
        else if at(i, "ch") { out.push_str("kh"); 2 }
        else if at(i, "eu") || at(i, "äu") { out.push_str("oy"); 2 }
        else if at(i, "ei") { out.push_str("eye"); 2 }
        else if at(i, "ie") { out.push_str("ee"); 2 }
        else if at(i, "qu") { out.push_str("kv"); 2 }
        else if i == 0 && at(i, "sp") || (i > 0 && !chars[i - 1].is_alphabetic() && at(i, "sp")) { out.push_str("shp"); 2 }
        else if i == 0 && at(i, "st") || (i > 0 && !chars[i - 1].is_alphabetic() && at(i, "st")) { out.push_str("sht"); 2 }
        else {
            match chars[i] {
                'ß' => out.push_str("ss"),
                'ä' => out.push_str("eh"),
                'ö' => out.push_str("er"),
                'ü' => out.push_str("ew"),
                'w' => out.push('v'),
                'z' => out.push_str("ts"),
                'v' => out.push('f'),
                'j' => out.push('y'),
                c => out.push(c),
            }
            1
        };
        i += step;
    }
    out
}

fn turkish(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        let s = match c {
            'ç' | 'Ç' => "ch", 'ş' | 'Ş' => "sh", 'ğ' | 'Ğ' => "", 'ı' | 'I' => "uh", 'İ' => "i",
            'ö' | 'Ö' => "er", 'ü' | 'Ü' => "ew", 'c' | 'C' => "j", 'j' | 'J' => "zh",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_is_transliterated() {
        assert_eq!(respell("привет").as_deref(), Some("preeveyt"));
    }

    #[test]
    fn korean_is_romanized() {
        assert_eq!(respell("사랑").as_deref(), Some("sarang"));
    }

    #[test]
    fn plain_english_needs_no_help() {
        assert_eq!(respell("hello there"), None);
    }
}
