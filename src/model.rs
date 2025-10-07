use super::lexer::Lexer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

type DocFreq = HashMap<String, usize>;
type TermFreq = HashMap<String, usize>;

#[derive(Deserialize, Serialize)]
pub struct Doc {
    tf: TermFreq,
    count: usize,
    last_modified: SystemTime,
}

type Docs = HashMap<PathBuf, Doc>;

#[derive(Default, Deserialize, Serialize)]
pub struct Model {
    pub docs: Docs,
    pub df: DocFreq,
}

impl Model {
    /// Remove a document from the index, decrementing document frequencies.
    fn remove_document(&mut self, path: &Path) {
        if let Some(doc) = self.docs.remove(path) {
            for term in doc.tf.keys() {
                if let Some(freq) = self.df.get_mut(term) {
                    *freq = freq.saturating_sub(1);
                }
            }
        }
    }

    /// Check whether the file at `path` needs re-indexing based on modification time.
    pub fn requires_reindexing(&self, path: &Path, last_modified: SystemTime) -> bool {
        self.docs
            .get(path)
            .is_none_or(|doc| doc.last_modified < last_modified)
    }

    /// Run a TF–IDF ranking over all indexed documents for the given query.
    pub fn search_query(&self, query: &str) -> Vec<(PathBuf, f32)> {
        // Tokenize and stem the query string
        let tokens: Vec<String> = Lexer::new(query).collect();

        // Compute TF–IDF score for each document
        let mut results: Vec<(PathBuf, f32)> = self
            .docs
            .iter()
            .filter_map(|(path, doc)| {
                let score: f32 = tokens
                    .iter()
                    .map(|t| compute_tf(t, doc) * compute_idf(t, self.docs.len(), &self.df))
                    .sum();

                if score.is_finite() && score > 0.0 {
                    Some((path.clone(), score))
                } else {
                    None
                }
            })
            .collect();

        // Sort descending by score
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        results
    }

    /// Add or update a document in the index, updating TF and DF maps.
    pub fn add_document(&mut self, path: PathBuf, last_modified: SystemTime, content: &str) {
        // Remove existing entry (decrements df)
        self.remove_document(&path);

        // Build term frequencies
        let tf: TermFreq = Lexer::new(content).fold(HashMap::new(), |mut acc, token| {
            *acc.entry(token).or_insert(0) += 1;
            acc
        });
        let count: usize = tf.values().sum();

        // Update document frequencies
        for term in tf.keys() {
            *self.df.entry(term.clone()).or_insert(0) += 1;
        }

        // Insert new document record
        let doc = Doc {
            tf,
            count,
            last_modified,
        };
        self.docs.insert(path, doc);
    }
}

/// Term frequency: term count divided by total terms in document.
fn compute_tf(term: &str, doc: &Doc) -> f32 {
    let total = doc.count as f32;
    let freq = *doc.tf.get(term).unwrap_or(&0) as f32;
    freq / total
}

/// Inverse document frequency: log10(N / df), with df at least 1.
fn compute_idf(term: &str, doc_count: usize, df: &DocFreq) -> f32 {
    let n = doc_count as f32;
    let m = *df.get(term).unwrap_or(&1) as f32;
    (n / m).log10()
}
