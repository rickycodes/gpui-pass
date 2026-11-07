use crate::{Error, Secret};
use std::sync::OnceLock;
use zeroize::Zeroizing;

const WORDS: &str = include_str!("../assets/eff-short-wordlist-2-0.txt");
const SEPARATORS: &[u8] = b";_-";
const ALPHANUMERIC: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const SYMBOLS: &[u8] = b"!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";
const MIN_WORDS: usize = 4;
const MAX_WORDS: usize = 8;
static WORD_LIST: OnceLock<Vec<&str>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneratorOptions {
    pub words: usize,
    pub add_digits: bool,
    pub add_separators: bool,
}

impl GeneratorOptions {
    pub fn entropy_bits(self) -> f64 {
        let word_bits = WORD_LIST.get_or_init(|| WORDS.lines().collect()).len() as f64;
        (self.words as f64 * word_bits.log2())
            + if self.add_digits {
                self.words as f64 * 10_f64.log2()
            } else {
                0.0
            }
            + if self.add_separators {
                self.words.saturating_sub(1) as f64 * (SEPARATORS.len() as f64).log2()
            } else {
                0.0
            }
    }
}

/// Generate a memorable phrase from uniformly selected EFF words and optional adornments.
pub fn generate_memorable_password(options: GeneratorOptions) -> Result<Secret, Error> {
    if !(MIN_WORDS..=MAX_WORDS).contains(&options.words) {
        return Err(Error::InvalidGenerationLength);
    }
    let words = WORD_LIST.get_or_init(|| WORDS.lines().collect::<Vec<_>>());
    if words.len() != 1296 {
        return Err(Error::InvalidWordList);
    }

    let mut password = Zeroizing::new(Vec::with_capacity(options.words * 12));
    for index in 0..options.words {
        if index > 0 && options.add_separators {
            let separator = SEPARATORS[random_below(SEPARATORS.len())?];
            password.push(separator);
        }
        password.extend_from_slice(words[random_below(words.len())?].as_bytes());
        if options.add_digits {
            password.push(b'0' + random_below(10)? as u8);
        }
    }
    Secret::from_bytes(password)
}

/// Generate with pass's standard default length and character set.
pub fn generate_pass_default_password() -> Result<Secret, Error> {
    let length = std::env::var("PASSWORD_STORE_GENERATED_LENGTH")
        .ok()
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| Error::InvalidGenerationLength)
        })
        .transpose()?
        .unwrap_or(25);
    if !(1..=MAX_GENERATED_LENGTH).contains(&length) {
        return Err(Error::InvalidGenerationLength);
    }
    let character_set = std::env::var("PASSWORD_STORE_CHARACTER_SET")
        .unwrap_or_else(|_| "[:punct:][:alnum:]".to_owned());
    let alphabet = parse_pass_character_set(&character_set)?;
    let mut password = Zeroizing::new(Vec::with_capacity(length));
    for _ in 0..length {
        password.push(alphabet[random_below(alphabet.len())?]);
    }
    Secret::from_bytes(password)
}

const MAX_GENERATED_LENGTH: usize = 1024;

fn parse_pass_character_set(pattern: &str) -> Result<Vec<u8>, Error> {
    // pass delegates this value to `tr -dc`; support its common POSIX classes and literal ASCII
    // patterns, while rejecting ranges/negation instead of guessing at shell/tr semantics.
    let mut alphabet = Vec::new();
    let mut remaining = pattern;
    while !remaining.is_empty() {
        if let Some(class) = remaining.strip_prefix("[:alnum:]") {
            alphabet.extend_from_slice(ALPHANUMERIC);
            remaining = class;
        } else if let Some(class) = remaining.strip_prefix("[:alpha:]") {
            alphabet.extend_from_slice(&ALPHANUMERIC[..52]);
            remaining = class;
        } else if let Some(class) = remaining.strip_prefix("[:lower:]") {
            alphabet.extend_from_slice(b"abcdefghijklmnopqrstuvwxyz");
            remaining = class;
        } else if let Some(class) = remaining.strip_prefix("[:upper:]") {
            alphabet.extend_from_slice(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ");
            remaining = class;
        } else if let Some(class) = remaining.strip_prefix("[:digit:]") {
            alphabet.extend_from_slice(b"0123456789");
            remaining = class;
        } else if let Some(class) = remaining.strip_prefix("[:punct:]") {
            alphabet.extend_from_slice(SYMBOLS);
            remaining = class;
        } else {
            let Some(character) = remaining.as_bytes().first().copied() else {
                break;
            };
            if !character.is_ascii_graphic() || matches!(character, b'-' | b'^' | b'[' | b']') {
                return Err(Error::InvalidGenerationLength);
            }
            alphabet.push(character);
            remaining = &remaining[1..];
        }
    }
    alphabet.sort_unstable();
    alphabet.dedup();
    if alphabet.is_empty() {
        Err(Error::InvalidGenerationLength)
    } else {
        Ok(alphabet)
    }
}

fn random_below(bound: usize) -> Result<usize, Error> {
    let bound = bound as u64;
    let threshold = bound.wrapping_neg() % bound;
    let mut sample = Zeroizing::new([0; 8]);
    loop {
        getrandom::fill(&mut sample[..]).map_err(|_| Error::RandomUnavailable)?;
        let value = u64::from_ne_bytes(*sample);
        if value >= threshold {
            return Ok((value % bound) as usize);
        }
    }
}
