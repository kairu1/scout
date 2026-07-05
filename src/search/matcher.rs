//! Fuzzy scoring behind a swappable trait (ADR-001 §Consequences,
//! Architect §4). `nucleo-matcher` is the v1 impl (ADR-002 slot 6); the
//! trait is the replaceability seam gate E requires.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher as Nucleo, Utf32Str};

/// One compiled query, scoring many candidates.
pub trait QueryScorer {
    /// `None` eliminates the candidate; `Some(score)` ranks it (higher
    /// is better; raw scale is matcher-defined).
    fn score(&mut self, candidate: &str) -> Option<u32>;

    /// Like `score`, but also yields the char indices of the matched
    /// positions (for render-side highlighting). Indices are into the
    /// candidate's char sequence, ascending, deduplicated.
    fn score_with_indices(&mut self, candidate: &str) -> Option<(u32, Vec<u32>)>;
}

pub trait Matcher {
    fn compile(&mut self, query: &str) -> Box<dyn QueryScorer + '_>;
}

pub struct NucleoMatcher {
    inner: Nucleo,
}

impl NucleoMatcher {
    pub fn new() -> Self {
        NucleoMatcher { inner: Nucleo::new(Config::DEFAULT.match_paths()) }
    }
}

impl Default for NucleoMatcher {
    fn default() -> Self {
        Self::new()
    }
}

struct NucleoQuery<'m> {
    matcher: &'m mut Nucleo,
    pattern: Pattern,
    buf: Vec<char>,
    indices: Vec<u32>,
}

impl QueryScorer for NucleoQuery<'_> {
    fn score(&mut self, candidate: &str) -> Option<u32> {
        let haystack = Utf32Str::new(candidate, &mut self.buf);
        self.pattern.score(haystack, self.matcher)
    }

    fn score_with_indices(&mut self, candidate: &str) -> Option<(u32, Vec<u32>)> {
        let haystack = Utf32Str::new(candidate, &mut self.buf);
        self.indices.clear();
        let score = self.pattern.indices(haystack, self.matcher, &mut self.indices)?;
        let mut indices = self.indices.clone();
        indices.sort_unstable();
        indices.dedup();
        Some((score, indices))
    }
}

impl Matcher for NucleoMatcher {
    fn compile(&mut self, query: &str) -> Box<dyn QueryScorer + '_> {
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        Box::new(NucleoQuery {
            matcher: &mut self.inner,
            pattern,
            buf: Vec::new(),
            indices: Vec::new(),
        })
    }
}
