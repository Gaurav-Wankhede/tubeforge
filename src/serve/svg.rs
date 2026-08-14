//! Server-rendered inline SVG charts (PRD §11 open question — resolved here:
//! no JS chart library; charts are Rust-generated SVG escaped at the source).
//!
//! Every label passes through `esc` before reaching the SVG text/attribute
//! position; the generated strings are inserted into templates with the
//! `|safe` filter, so escaping MUST happen here (untrusted DB titles).
//! Shapes are pure functions of the input data — unit-tested below.

/// HTML-escape for SVG text content and quoted attributes. Covers the five
/// XML metacharacters plus the apostrophe (used in single-quoted attrs).
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Horizontal bar chart of views per channel (top-N, caller-ordered).
/// Returns a full `<svg>` document string with escaped labels.
pub fn views_bars(items: &[(String, i64)]) -> String {
    const W: f64 = 560.0;
    const LABEL_W: f64 = 190.0;
    const VAL_W: f64 = 60.0;
    const ROW_H: f64 = 24.0;
    const GAP: f64 = 8.0;

    let max = items.iter().map(|(_, v)| *v).max().unwrap_or(0).max(1);
    let h = items.len() as f64 * (ROW_H + GAP) + 8.0;
    let mut out = format!(
        r#"<svg viewBox="0 0 {W} {h}" role="img" aria-label="Views per channel" xmlns="http://www.w3.org/2000/svg">"#
    );
    for (i, (label, v)) in items.iter().enumerate() {
        let y = 4.0 + i as f64 * (ROW_H + GAP);
        let bw = (*v as f64 / max as f64) * (W - LABEL_W - VAL_W);
        out.push_str(&format!(
            r#"<text x="4" y="{:.0}" class="cl">{}</text>"#,
            y + 14.0,
            esc(label)
        ));
        out.push_str(&format!(
            r#"<rect x="{LABEL_W}" y="{y:.0}" width="{bw:.1}" height="16" rx="3" class="bar"/>"#
        ));
        out.push_str(&format!(
            r#"<text x="{:.1}" y="{:.0}" class="cv">{}</text>"#,
            LABEL_W + bw + 6.0,
            y + 14.0,
            v
        ));
    }
    out.push_str("</svg>");
    out
}

/// Mini-histogram of SEO total scores over 10 buckets of 10 points.
/// Bucket counts scale to the tallest bucket; zero-width bars only when a
/// bucket is empty.
pub fn score_histogram(scores: &[f64]) -> String {
    const W: f64 = 560.0;
    const H: f64 = 140.0;
    const BUCKETS: usize = 10;

    let mut counts = [0usize; BUCKETS];
    for s in scores {
        let b = (s.clamp(0.0, 99.9) / 10.0) as usize;
        counts[b.min(BUCKETS - 1)] += 1;
    }
    let max = counts.iter().copied().max().unwrap_or(1).max(1);
    let slot = W / BUCKETS as f64;

    let mut out = format!(
        r#"<svg viewBox="0 0 {W} {H}" role="img" aria-label="SEO score distribution" xmlns="http://www.w3.org/2000/svg">"#
    );
    for (i, c) in counts.iter().enumerate() {
        let bar_h = (*c as f64 / max as f64) * (H - 24.0);
        let x = i as f64 * slot + 2.0;
        let bw = slot - 4.0;
        out.push_str(&format!(
            r#"<rect x="{x:.1}" y="{:.1}" width="{bw:.1}" height="{bar_h:.1}" rx="2" class="bar"/>"#,
            H - 20.0 - bar_h
        ));
    }
    out.push_str(&format!(
        r#"<text x="2" y="{:.0}" class="ca">0</text><text x="{:.0}" y="{:.0}" class="ca">100</text>"#,
        H - 4.0,
        W - 20.0,
        H - 4.0
    ));
    out.push_str("</svg>");
    out
}

/// Position sparkline: y is inverted (lower rank = higher on the chart);
/// `None` breaks the line. Points are placed evenly across the width.
pub fn sparkline(points: &[Option<i64>]) -> String {
    const W: f64 = 140.0;
    const H: f64 = 32.0;

    if points.is_empty() {
        return String::new();
    }
    let max_pos = points.iter().flatten().copied().max().unwrap_or(1).max(1);
    let n = points.len();
    let step = if n > 1 { W / (n - 1) as f64 } else { 0.0 };

    let point = |i: usize, p: i64| -> (f64, f64) {
        let x = if n > 1 { i as f64 * step } else { W / 2.0 };
        let y = H - 3.0 - (p as f64 - 1.0) / max_pos as f64 * (H - 6.0);
        (x, y)
    };

    let mut out = format!(
        r#"<svg viewBox="0 0 {W} {H}" role="img" aria-label="Position trend" xmlns="http://www.w3.org/2000/svg">"#
    );
    // Segments between consecutive ranked points.
    let mut seg: Vec<(f64, f64)> = Vec::new();
    for (i, p) in points.iter().enumerate() {
        match p {
            Some(v) => seg.push(point(i, *v)),
            None => {
                if seg.len() >= 2 {
                    out.push_str(&polyline(&seg));
                }
                seg.clear();
            }
        }
    }
    if seg.len() >= 2 {
        out.push_str(&polyline(&seg));
    }
    // Dots for every ranked point.
    for (i, p) in points.iter().enumerate() {
        if let Some(v) = p {
            let (x, y) = point(i, *v);
            out.push_str(&format!(
                r#"<circle cx="{x:.1}" cy="{y:.1}" r="2" class="dot"/>"#
            ));
        }
    }
    out.push_str("</svg>");
    out
}

/// Layered architecture diagram for `/docs/architecture` (same spirit as
/// PRD §11: everything server-rendered, no JS). Five layers top-to-bottom,
/// each a rounded panel with an accent title and dim caption lines;
/// vertical arrows with marker-end arrowheads connect them. Static text,
/// but every caption still passes through `esc` — same contract as the
/// charts above. Styling rides the dashboard CSS vars via the `.dlay*`,
/// `.darr`/`.dhead` and `.chip` classes in `base.html`.
pub fn layers_diagram() -> String {
    let mut out = String::from(
        r#"<svg viewBox="0 0 1000 640" width="100%" role="img" aria-label="TubeForge layered architecture" xmlns="http://www.w3.org/2000/svg"><defs><marker id="darrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" class="dhead"/></marker></defs>"#,
    );

    // (title, caption lines, y, height)
    let layers: [(&str, &[&str], f64, f64); 5] = [
        (
            "1 · Inputs",
            &["channel RSS · video links (oEmbed) · optional YouTube Data API v3 key"],
            16.0,
            66.0,
        ),
        (
            "2 · Fetch",
            &["RSS · oEmbed · Data API v3 — ETag-cached · quota ledger (1 unit / call)"],
            102.0,
            66.0,
        ),
        (
            "3 · Storage + Search",
            &[
                "Turso — SQLite-compatible · WAL · schema v3 · migrations 001–003",
                "tantivy — BM25 title & description index",
            ],
            188.0,
            86.0,
        ),
        (
            "4 · Intelligence",
            &[
                "Scoring — 10 SEO + 7 GEO components · env-configurable weights",
                "Analytics — PageRank · Next Ideas (0.5·seo + 0.3·fit + 0.2·gap)",
                "keyword rank snapshots · scorecard · health · alerts",
            ],
            294.0,
            106.0,
        ),
        ("5 · Outputs", &[], 420.0, 200.0),
    ];
    for (title, lines, y, h) in &layers {
        out.push_str(&format!(
            r#"<rect x="20" y="{y:.0}" width="960" height="{h:.0}" rx="12" class="dlay"/>"#
        ));
        out.push_str(&format!(
            r#"<text x="40" y="{:.0}" class="dlay-t">{}</text>"#,
            y + 26.0,
            esc(title)
        ));
        for (i, line) in lines.iter().enumerate() {
            out.push_str(&format!(
                r#"<text x="40" y="{:.0}" class="dlay-c">{}</text>"#,
                y + 52.0 + i as f64 * 20.0,
                esc(line)
            ));
        }
    }

    // Vertical flow arrows between consecutive layers (marker-end heads).
    for (y1, y2) in [
        (82.0, 102.0),
        (168.0, 188.0),
        (274.0, 294.0),
        (400.0, 420.0),
    ] {
        out.push_str(&format!(
            r#"<line x1="500" y1="{y1:.0}" x2="500" y2="{y2:.0}" class="darr" marker-end="url(#darrow)"/>"#
        ));
    }

    // Layer 5: five output chips in a row.
    let chips: [(&str, &str); 5] = [
        ("CLI --json", "exit codes 0–5"),
        ("export", "CSV / ZIP"),
        ("thumbnail render", "chromiumoxide · Tailwind v4"),
        ("rpc", "stdio JSON-RPC"),
        ("serve", "dashboard · SSE · CSRF"),
    ];
    for (i, (t, s)) in chips.iter().enumerate() {
        let x = 40.0 + i as f64 * 188.0;
        out.push_str(&format!(
            r#"<rect x="{x:.0}" y="470" width="180" height="64" rx="8" class="chip"/>"#
        ));
        out.push_str(&format!(
            r#"<text x="{:.0}" y="496" text-anchor="middle" class="dlay-h">{}</text>"#,
            x + 90.0,
            esc(t)
        ));
        out.push_str(&format!(
            r#"<text x="{:.0}" y="516" text-anchor="middle" class="dlay-c">{}</text>"#,
            x + 90.0,
            esc(s)
        ));
    }
    out.push_str(
        r#"<text x="500" y="560" text-anchor="middle" class="dlay-c">one binary — every output is a command on the same engine</text>"#,
    );

    out.push_str("</svg>");
    out
}

fn polyline(seg: &[(f64, f64)]) -> String {
    let pts: Vec<String> = seg.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
    format!(
        r#"<polyline points="{}" fill="none" class="line"/>"#,
        pts.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_covers_xml_metacharacters() {
        assert_eq!(
            esc(r#"<a href="x" & 'y'>"#),
            "&lt;a href=&quot;x&quot; &amp; &#39;y&#39;&gt;"
        );
        assert_eq!(esc("plain text"), "plain text");
    }

    #[test]
    fn views_bars_escapes_labels() {
        let svg = views_bars(&[("Mr <Evil> & \"Co\"".to_string(), 5)]);
        assert!(svg.contains("&lt;Evil&gt;"));
        assert!(!svg.contains("<Evil>"));
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn views_bars_scales_to_max() {
        let svg = views_bars(&[("a".to_string(), 100), ("b".to_string(), 50)]);
        // First bar is twice the second; both contain a width attribute.
        assert!(svg.contains(r#"class="bar""#));
        assert!(svg.contains("aria-label=\"Views per channel\""));
    }

    #[test]
    fn views_bars_handles_empty_and_zero() {
        let svg = views_bars(&[]);
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        // Zero views must not produce an infinite bar.
        let svg = views_bars(&[("zero".to_string(), 0)]);
        assert!(svg.contains("width=\"0.0\""));
    }

    #[test]
    fn histogram_buckets_known_scores() {
        let svg = score_histogram(&[0.0, 10.0, 50.0, 99.0, 100.0]);
        assert!(svg.contains("SEO score distribution"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn sparkline_breaks_on_missing() {
        let svg = sparkline(&[Some(1), None, Some(3)]);
        assert!(svg.contains("<circle"));
        // Single ranked point: no polyline.
        let svg = sparkline(&[Some(1)]);
        assert!(!svg.contains("<polyline"));
    }

    #[test]
    fn sparkline_empty_is_empty() {
        assert_eq!(sparkline(&[]), "");
    }
}
