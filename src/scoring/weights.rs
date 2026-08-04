//! Weight configuration (LLD §7.4, §11).
//!
//! Precedence: env overrides > baked defaults. `settings_json` overrides are
//! deferred (LLD mentions them; Phase 2 scope is env keys only).
//!
//! Keys:
//! - `TUBEFORGE_WEIGHTS_SEO` / `TUBEFORGE_WEIGHTS_GEO` — composite group
//!   weights (default `1.0` / `1.0`, LLD §11).
//! - `TUBEFORGE_SEO_<COMPONENT>` / `TUBEFORGE_GEO_<COMPONENT>` — per-component
//!   weights, uppercase snake of the JSON name, e.g. `TUBEFORGE_SEO_KEYWORD_TITLE`.
//!
//! Sum-normalization happens at use time (LLD §7.4:
//! `seo_total = Σ(w_i · comp_i) / Σ w_i`), so overrides need not sum to 1.

use std::collections::HashMap;

use crate::error::TubeforgeError;

/// SEO components in canonical order (LLD §7.2 table; `keyword_tags` is the
/// Phase 1 basic-mode signal retained alongside the Phase 2 set).
pub const SEO_COMPONENTS: [&str; 10] = [
    "keyword_title",
    "title_front",
    "title_length",
    "title_hooks",
    "keyword_desc",
    "desc_first150",
    "desc_structure",
    "tags_relevance",
    "tags_quality",
    "keyword_tags",
];

/// GEO components in canonical order (LLD §7.3 + C1/C2 free signals).
pub const GEO_COMPONENTS: [&str; 7] = [
    "entity_coverage",
    "qa_phrasing",
    "list_phrasing",
    "conversational",
    "metadata_complete",
    "location_signal",
    "topic_relevance",
];

/// Documented baked SEO defaults (sum 1.00).
const DEFAULT_SEO: [(&str, f64); 10] = [
    ("keyword_title", 0.25),
    ("title_front", 0.10),
    ("title_length", 0.10),
    ("title_hooks", 0.05),
    ("keyword_desc", 0.15),
    ("desc_first150", 0.10),
    ("desc_structure", 0.05),
    ("tags_relevance", 0.10),
    ("tags_quality", 0.05),
    ("keyword_tags", 0.05),
];

/// Documented baked GEO defaults (sum 1.00). The C1/C2 free signals
/// (`location_signal`, `topic_relevance`) get 0.10 each; the five Phase 2
/// components scale to 0.80 so the set re-normalizes to exactly 1.0.
const DEFAULT_GEO: [(&str, f64); 7] = [
    ("entity_coverage", 0.24),
    ("qa_phrasing", 0.12),
    ("list_phrasing", 0.12),
    ("conversational", 0.16),
    ("metadata_complete", 0.16),
    ("location_signal", 0.10),
    ("topic_relevance", 0.10),
];

/// Resolved weight set: group weights + per-component weights.
#[derive(Debug, Clone)]
pub struct Weights {
    pub seo_group: f64,
    pub geo_group: f64,
    seo: HashMap<String, f64>,
    geo: HashMap<String, f64>,
}

impl Weights {
    /// Baked defaults only (documented above).
    pub fn defaults() -> Self {
        Weights {
            seo_group: 1.0,
            geo_group: 1.0,
            seo: DEFAULT_SEO.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            geo: DEFAULT_GEO.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        }
    }

    /// Defaults overlaid with env overrides. Bad values are a `Config` error.
    pub fn from_env() -> Result<Self, TubeforgeError> {
        let mut w = Weights::defaults();
        w.seo_group = env_f64("TUBEFORGE_WEIGHTS_SEO", w.seo_group)?;
        w.geo_group = env_f64("TUBEFORGE_WEIGHTS_GEO", w.geo_group)?;
        for key in SEO_COMPONENTS {
            w.seo.insert(
                key.to_string(),
                env_f64(&env_key("TUBEFORGE_SEO_", key), w.seo[key])?,
            );
        }
        for key in GEO_COMPONENTS {
            w.geo.insert(
                key.to_string(),
                env_f64(&env_key("TUBEFORGE_GEO_", key), w.geo[key])?,
            );
        }
        Ok(w)
    }

    pub fn seo_weight(&self, key: &str) -> f64 {
        self.seo.get(key).copied().unwrap_or(0.0)
    }

    pub fn geo_weight(&self, key: &str) -> f64 {
        self.geo.get(key).copied().unwrap_or(0.0)
    }

    /// Sum of the SEO component weights (used for normalization).
    pub fn seo_sum(&self) -> f64 {
        self.seo.values().sum()
    }

    /// Sum of the GEO component weights (used for normalization).
    pub fn geo_sum(&self) -> f64 {
        self.geo.values().sum()
    }
}

/// `TUBEFORGE_SEO_KEYWORD_TITLE` for `keyword_title`.
fn env_key(prefix: &str, component: &str) -> String {
    format!("{prefix}{}", component.to_ascii_uppercase())
}

/// Read an env var as a non-negative f64, falling back to `default`.
fn env_f64(var: &str, default: f64) -> Result<f64, TubeforgeError> {
    let Ok(raw) = std::env::var(var) else {
        return Ok(default);
    };
    let v: f64 = raw
        .parse()
        .map_err(|_| TubeforgeError::Config(format!("{var} not a number: {raw:?}")))?;
    if v < 0.0 {
        return Err(TubeforgeError::Config(format!(
            "{var} must be >= 0 (negative weights are meaningless)"
        )));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Env vars are process-global; parallel tests would race on them. Every
    /// env-mutating test holds this lock for its whole body.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn defaults_sum_to_one_per_group() {
        let w = Weights::defaults();
        assert!((w.seo_sum() - 1.0).abs() < 1e-9, "seo sum = {}", w.seo_sum());
        assert!((w.geo_sum() - 1.0).abs() < 1e-9, "geo sum = {}", w.geo_sum());
        assert_eq!(w.seo_group, 1.0);
        assert_eq!(w.geo_group, 1.0);
        // Every component key resolves.
        for k in SEO_COMPONENTS {
            assert!(w.seo_weight(k) > 0.0, "seo {k} has a weight");
        }
        for k in GEO_COMPONENTS {
            assert!(w.geo_weight(k) > 0.0, "geo {k} has a weight");
        }
    }

    /// Env overrides + defaults. Kept in ONE test fn (plus lock): env vars
    /// are process global, so parallel tests would race on them.
    #[test]
    fn env_overrides_and_defaults() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _guard = EnvGuard::set(&[
            ("TUBEFORGE_WEIGHTS_SEO", "0.7"),
            ("TUBEFORGE_WEIGHTS_GEO", "1.3"),
            ("TUBEFORGE_SEO_KEYWORD_TITLE", "0.5"),
            ("TUBEFORGE_GEO_ENTITY_COVERAGE", "0.9"),
            ("TUBEFORGE_SEO_NO_SUCH", "0.1"), // ignored: unknown key
        ]);

        let w = Weights::from_env().expect("parse");
        assert_eq!(w.seo_group, 0.7);
        assert_eq!(w.geo_group, 1.3);
        assert_eq!(w.seo_weight("keyword_title"), 0.5);
        assert_eq!(w.geo_weight("entity_coverage"), 0.9);
        // Unset components keep their baked defaults.
        assert_eq!(w.seo_weight("title_front"), 0.10);
        assert_eq!(w.geo_weight("qa_phrasing"), 0.12);
        assert_eq!(w.geo_weight("location_signal"), 0.10);
        assert_eq!(w.geo_weight("topic_relevance"), 0.10);
    }

    #[test]
    fn env_bad_number_is_config_error() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _guard = EnvGuard::set(&[("TUBEFORGE_SEO_KEYWORD_TITLE", "not-a-number")]);
        let err = Weights::from_env().expect_err("bad env value");
        assert!(matches!(err, TubeforgeError::Config(_)), "got {err:?}");
    }

    #[test]
    fn env_negative_is_config_error() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _guard = EnvGuard::set(&[("TUBEFORGE_WEIGHTS_SEO", "-1.0")]);
        let err = Weights::from_env().expect_err("negative env value");
        assert!(matches!(err, TubeforgeError::Config(_)), "got {err:?}");
    }

    /// Sets env vars for the duration of one test, restoring on drop.
    struct EnvGuard(Vec<(String, Option<String>)>);

    impl EnvGuard {
        fn set(kvs: &[(&str, &str)]) -> Self {
            let mut saved = Vec::new();
            for (k, v) in kvs {
                saved.push((k.to_string(), std::env::var(k).ok()));
                std::env::set_var(k, v);
            }
            EnvGuard(saved)
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, prev) in &self.0 {
                match prev {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}
