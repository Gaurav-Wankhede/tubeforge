# yt-dlp `extract_info` JSON Schema Reference

**Captured:** 2026-08-05, from `yt_dlp.YoutubeDL(opts).extract_info(url, download=False)` against a live public video (`3e-nauaCkgo` — "Rust is the New C", No Boilerplate) using the `android` player client (keyless).

**Purpose:** authoritative reference for designing TubeForge's yt-dlp integration (Phase 6.5). Every key below was observed in a real extraction; types and shapes are as emitted by yt-dlp 2025.10.14 (Python 3.9 venv).

**Source file:** `/tmp/extract-info-full.json` (523 KB raw dump).

---

## Top-level keys (82 total)

### Identity & media
| Key | Type | Example |
|---|---|---|
| `id` | str | `3e-nauaCkgo` |
| `title` | str | `Rust is the New C` |
| `fulltitle` | str | `Rust is the New C` |
| `display_id` | str | `3e-nauaCkgo` |
| `description` | str | long text |
| `duration` | int (seconds) | `652` |
| `duration_string` | str | `10:52` |
| `media_type` | str | `video` |
| `thumbnail` | str (URL) | `https://i.ytimg.com/vi_webp/.../maxresdefault.webp` |
| `thumbnails` | array\<object\> | 42 entries (resolution variants) |
| `webpage_url` / `original_url` | str | watch URL |
| `webpage_url_domain` | str | `youtube.com` |

### Channel
| Key | Type | Example |
|---|---|---|
| `channel` | str | `No Boilerplate` |
| `channel_id` | str | `UCUMwY9iS8oMyWDYIe6_RmoA` |
| `channel_url` | str | `https://www.youtube.com/channel/...` |
| `channel_follower_count` | int | `289000` |
| `channel_is_verified` | bool | — |
| `uploader` / `uploader_id` / `uploader_url` | str | alias of channel |
| `creators` | array\<str\> | collaborators (may be None) |

### Statistics
| Key | Type | Example |
|---|---|---|
| `view_count` | int | `172452` |
| `like_count` | int | `9973` |
| `comment_count` | int | `1500` |
| `average_rating` | null | not exposed |
| `age_limit` | int | `0` |
| `availability` | str | `public` |
| `is_live` / `was_live` | bool | `false` |
| `live_status` | str | `not_live` |
| `playable_in_embed` | bool | `true` |

### Dates
| Key | Type | Example |
|---|---|---|
| `timestamp` | int (unix) | `1741780749` |
| `upload_date` | str `YYYYMMDD` | `20250312` |
| `release_timestamp` | int/null | — |
| `release_year` | int/null | — |
| `modified_date` | str/null | — |

### Taxonomy & keywords
| Key | Type | Example |
|---|---|---|
| `categories` | array\<str\> | `["Education"]` |
| `tags` | array\<str\> | **`[]` on android client** (YouTube strips keywords from mobile player responses; populated on `web` client with PO tokens) |

### Engagement (unique to yt-dlp — NOT in the YouTube Data API)
| Key | Type | Example |
|---|---|---|
| `heatmap` | array\<object\> (100 pts) | `{"start_time": 0.0, "end_time": 6.52, "value": 0.337}` — audience-retention curve: `{start_time, end_time, value}` |

### Captions
| Key | Type | Example |
|---|---|---|
| `automatic_captions` | object: lang → array\<track\> | 157 langs; `en` has 7 tracks (json3, srv1-3, ttml, srt, vtt) |
| `subtitles` | object | manual subs (may be `{}`) |

Track item shape: `{ext, url, name, is_automatic, language, kind}` — `ext` ∈ {json3, srv1, srv2, srv3, ttml, srt, vtt}.

### Playlist context
| Key | Type | Example |
|---|---|---|
| `playlist` | null/str | — |
| `playlist_index` | null/int | — |

### Formats (download layer — not needed for metadata mining)
| Key | Type |
|---|---|
| `formats` | array\<object\> (5 items, android) |
| per-format keys | `format_id, url, ext, width, height, fps, acodec, vcodec, protocol, quality, dynamic_range, tbr, asr, audio_channels, abr, filesize, http_headers, format_note, source_preference, has_drm, language_preference, preference, aspect_ratio, resolution, format` |
| `requested_formats`, `_format_sort_fields`, `_format_sort_string`, `_has_drm` | internal |

### Internal metadata (underscore-prefixed)
| Key | Type |
|---|---|
| `_type` | str (`video` / `url` for flat) |
| `_filename` | str |
| `_version` | object |
| `epoch` | int |
| `extractor` / `extractor_key` | str (`youtube`) |
| `requested_subtitles` | object/null |
| `available_at` | int |

---

## Design implications for TubeForge

1. **Strict schema mapping** (`YtdlpVideoInfo`): consume only the whitelisted keys above; unknown keys must never break the pipeline (YouTube/yt-dlp field drift is expected). Observed payloads contain format-layer noise — filter it out.

2. **Tags caveat (empirically confirmed):** `tags` is empty on the `android`/`android_vr` clients. The `web` client (with PO-token JS runtime) is required for keywords. Design must treat tags as best-effort: yt-dlp → empty = degrade gracefully, not error.

3. **`heatmap`** is a TubeForge-only signal (100-point retention curve) — store it (`video_heatmap` or JSON column). The YouTube Data API cannot provide it.

4. **Captions:** `automatic_captions` gives track URLs per language; `en` has 7 formats. TubeForge's VTT parser already handles the `vtt` track (verified against a real captured file). Prefer `vtt` for transcript extraction.

5. **Timestamp handling:** use `timestamp` (unix int) or `upload_date` (YYYYMMDD) — both present; `published_at` in TubeForge's `videos` table should map from `timestamp` as RFC3339.

6. **Channel data:** `channel_follower_count` (subs) and `channel_is_verified` are free here but absent from RSS — a yt-dlp enrichment pass can backfill them into the `channels` table.

7. **Rate/bot reality (this machine, 2026-08-05):** `web` client → "The page needs to be reloaded" without PO tokens; `android` works keyless. Client selection MUST be configurable (`TUBEFORGE_YTDLP_CLIENT`), defaulting to yt-dlp's native multi-client chain with `all` as a robust fallback.
