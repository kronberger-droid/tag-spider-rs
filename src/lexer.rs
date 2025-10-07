use rust_stemmers::{Algorithm, Stemmer};
use std::iter::Peekable;
use std::str::CharIndices;

pub struct Lexer<'a> {
    input: &'a str,
    chars: Peekable<CharIndices<'a>>,
    start: usize,
    end: usize,
    stemmer: Stemmer,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input,
            chars: input.char_indices().peekable(),
            start: 0,
            end: 0,
            stemmer: Stemmer::create(Algorithm::English),
        }
    }

    /// Skips any leading whitespace characters.
    fn trim_left(&mut self) {
        while let Some(&(_, ch)) = self.chars.peek() {
            if ch.is_whitespace() {
                self.chars.next();
            } else {
                break;
            }
        }
    }

    /// Returns the slice of the current token.
    fn slice_current(&self) -> &'a str {
        &self.input[self.start..self.end]
    }

    /// Consumes characters while `cond` holds, updating `start` and `end`.
    fn chop_while<F>(&mut self, mut cond: F)
    where
        F: FnMut(char) -> bool,
    {
        if let Some((idx, ch)) = self.chars.next() {
            self.start = idx;
            self.end = idx + ch.len_utf8();

            if cond(ch) {
                while let Some(&(_, next_ch)) = self.chars.peek() {
                    if cond(next_ch) {
                        let (idx, next_ch) = self.chars.next().unwrap();
                        self.end = idx + next_ch.len_utf8();
                    } else {
                        break;
                    }
                }
            }
        }
    }

    /// Retrieves the next token, performing stemming on alphabetic runs.
    pub fn next_token(&mut self) -> Option<String> {
        self.trim_left();
        let &(_, ch) = self.chars.peek()?;

        if ch.is_ascii_digit() {
            self.chop_while(|c| c.is_ascii_digit());
            return Some(self.slice_current().to_string());
        }

        if ch.is_alphabetic() {
            self.chop_while(|c| c.is_alphanumeric());
            let term = self.slice_current().to_lowercase();
            let stemmed = self.stemmer.stem(&term).to_string();
            return Some(stemmed);
        }

        // Any other single character (punctuation, symbol)
        if let Some((idx, ch)) = self.chars.next() {
            self.start = idx;
            self.end = idx + ch.len_utf8();
            return Some(ch.to_string());
        }

        None
    }
}

impl Iterator for Lexer<'_> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}
