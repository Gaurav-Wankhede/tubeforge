# Complete yt-dlp Metadata Schema Specification

The full raw JSON extraction from live YouTube execution is saved at `docs/YTDLP_METADATA_SCHEMA.json`.

---

## 1. Top-Level Attribute Taxonomy (77 Extracted Fields)

yt-dlp extracts **77 core metadata attributes** categorized into 7 functional domains:

```
┌─────────────────────────────────────────────────────────────────────────────────────────────┐
│                                YT-DLP METADATA TAXONOMY                                     │
├──────────────────────────────────────┬──────────────────────────────────────────────────────┤
│ Domain                               │ Key Metadata Fields Extracted                        │
├──────────────────────────────────────┼──────────────────────────────────────────────────────┤
│ 1. Core Identification & Basic SEO   │ id, title, fulltitle, description, tags, categories  │
│ 2. Real-Time Performance & Metrics   │ view_count, like_count, comment_count, timestamp    │
│ 3. Channel & Creator Authority       │ channel, channel_id, channel_url, channel_followers  │
│ 4. Structural Video Timeline         │ duration, duration_string, chapters (timestamps)     │
│ 5. Visual Assets & Heatmap           │ thumbnail, thumbnails (array of sizes), heatmap      │
│ 6. Semantic Captions & Transcripts   │ subtitles, automatic_captions (VTT/SRT/JSON3)        │
│ 7. Technical Stream Specs            │ resolution, width, height, fps, vcodec, acodec, tbr  │
└──────────────────────────────────────┴──────────────────────────────────────────────────────┘
```

---

## 2. Categorized Field Breakdown & Data Science Utility

### A. Core Video & Content Metadata

| JSON Key | Type | Example Value | TubeForge SEO Utility |
|---|---|---|---|
| `id` | `String` | `"vN_7i0Vxt4s"` | Primary Key (`videos.video_id`) |
| `title` | `String` | `"Rust Ownership & Borrowing: 3 Rules..."` | Mobile 45-character hook analysis & zero-colon gate |
| `fulltitle` | `String` | `"Rust Ownership & Borrowing: 3 Rules..."` | Verbatim title comparison |
| `description` | `String` | `"Learn Rust ownership and borrowing..."` | First-150-char hook, keyword density, chapter links |
| `tags` | `Array<String>` | `["rust ownership", "borrow checker"]` | Tag gap mining, keyword radar, and competitor overlap |
| `categories` | `Array<String>` | `["Science & Technology"]` | YouTube topical categorization classification |

---

### B. Live Audience Engagement & Time-Series Signals

| JSON Key | Type | Example Value | TubeForge Data Science Formula |
|---|---|---|---|
| `view_count` | `Integer` | `11` | Baseline scale for Outlier Multiplier $R = \frac{V}{\mu_{chan}}$ |
| `like_count` | `Integer` | `3` | Engagement Density numerator component |
| `comment_count` | `Integer` | `2` | High-intent explicit audience feedback ($2.5\times$ weight) |
| `timestamp` | `Integer` | `1784892602` | Epoch creation time for velocity $\Delta V / \Delta t$ |
| `upload_date` | `String` | `"20260724"` | `YYYYMMDD` formatted upload date |

---

### C. Channel & Creator Authority Footprint

| JSON Key | Type | Example Value | TubeForge Utility |
|---|---|---|---|
| `channel` | `String` | `"Gaurav Wankhede - TECHVERSE"` | Channel title |
| `channel_id` | `String` | `"UC4BK6cXh5id7rG_k-rUqQTA"` | Foreign Key (`channels.channel_id`) |
| `channel_url` | `String` | `"https://youtube.com/@..."` | Canonical channel link |
| `channel_follower_count` | `Integer` | `51` | Subscriber baseline for subscriber-to-view ratio |
| `uploader_id` | `String` | `"@GauravWankhede-TECHVERSE"` | YouTube `@handle` |

---

### D. Video Structure, Chapters & Retention Heatmap

| JSON Key | Type | Example Value | TubeForge Utility |
|---|---|---|---|
| `duration` | `Integer` | `373` | Total duration in seconds (Expected Watch Time) |
| `duration_string` | `String` | `"6:13"` | Formatted duration stamp |
| `chapters` | `Array<Object>` | `[{"start_time": 0, "title": "Intro"}]` | Retention breakdown, chapter density, structure score |
| `heatmap` | `Array<Object>` | `[{"start_time": 12.0, "value": 0.85}]` | Exact YouTube player rewind & retention spikes |

---

### E. Semantic Transcripts & Captions

| JSON Key | Type | Example Value | TubeForge Utility |
|---|---|---|---|
| `automatic_captions` | `Map<Lang, Formats>` | `{ "en": [{ "ext": "vtt", "url": "..." }] }` | Ingests spoken content for semantic search & RAG |
| `subtitles` | `Map<Lang, Formats>` | `{}` | Verified manual subtitles |

---

### F. Visual Thumbnails & Technical Media Specifications

| JSON Key | Type | Example Value | TubeForge Utility |
|---|---|---|---|
| `thumbnail` | `String` | `"https://i.ytimg.com/vi/.../maxres.jpg"` | HD thumbnail preview in Personal Studio |
| `thumbnails` | `Array<Object>` | `[{"url": "...", "width": 1920, "height": 1080}]` | Resolution picker for maximum clarity |
| `resolution` | `String` | `"1920x1080"` | Quality tier check |
| `fps` | `Integer` | `60` | Frame rate evaluation |
| `vcodec` | `String` | `"avc1.64002a"` | Video codec (H.264, VP9, AV1) |
| `acodec` | `String` | `"opus"` | Audio stream codec (Opus, AAC) |
