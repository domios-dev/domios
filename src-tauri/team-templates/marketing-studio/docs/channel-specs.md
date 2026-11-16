# Channel specs

The channels that mattered for a developer product launch, in order, with the rules followed on each. Hacker News, dev.to and newsletters are partly from experience elsewhere rather than this launch. Every post on every channel waits for the owner's OK (docs/launch-playbook.md).

## Product Hunt

- Launch at 00:01 Pacific time; the day runs on Pacific time.
- Gallery images 1270x760, thumbnail 240x240, a YouTube video link.
- Write a maker comment.
- Never ask for upvotes and never buy votes. Upvote sellers will email you; the answer is no.
- The official badge embed shows a live vote count. Use a static, self-hosted badge instead: a low counter hurts.

## GitHub README

The real landing page for developers. It needs a one-line value proposition, a GIF, install steps and badges. The site carries a "Star us on GitHub" call to action.

## Hacker News (Show HN)

- A plain, technical title with no marketing words.
- The maker must be present to answer questions.
- Never ask for votes; vote rings get flagged.
- The owner posts personally.

## Reddit

- Read each subreddit's self-promotion rules first and obey them.
- Lead with value or a story, not a link dump.
- Post from the owner's own aged account only.

## X and LinkedIn

- The owner's voice, and it must not read as AI. Follow the owner's social-media or humanizer rules when they have them; otherwise edit as "Owner voice without fake humanity" in docs/launch-playbook.md describes: remove the tells, never add invented specifics or a borrowed personality.
- Stylized stories are fine.
- One image or one short video, 9:16 or 16:9.

## Directories

- Cheap backlinks.
- Before submitting, check each one's terms: badge requirements, free-tier waiting weeks, paid upsells.
- Track every one in the listings registry, with its deadlines.

## Discord

An invite on the site, and a moderated server.

## dev.to and newsletters

- Repurpose long-form "how we built it" posts for dev.to.
- Pitch newsletter editors directly; never pay for a feature presented as not sponsored.

## Site and search

Checks for the landing page and every page the launch links to, before the owner sees the preview. Each item is measured on the rendered page, not assumed from the source.

- **Title and description:** a unique title of about 60 characters at most and a description of about 155 at most, both from the messaging doc, with the product name and what it does. Longer ones get cut off in results.
- **Structure:** one H1 that says what the page is, headings in order, alt text from the messaging doc.
- **Canonical and indexing:** a canonical URL on every page; `robots.txt` and meta robots do not block the pages meant to be found; a sitemap lists them.
- **Link previews:** Open Graph and X card tags with the 1200x630 image, checked in a preview validator or by pasting the link into a private draft.
- **Structured data:** `SoftwareApplication` or `Organization` markup only with true values. Never add ratings, reviews or counts that do not exist.
- **One page per query:** two pages aimed at the same search (two "install" pages, an old and a new landing page) compete with each other. Merge them, or give each a distinct purpose and title.
- **Freshness:** sweep for old dates, version numbers, statistics and old names before every launch moment.
- **What was not measured:** say so. Rankings, search volume and click-through need the owner's analytics access; without it they are missing, not estimated.

No power words, decorative symbols, emoji or year-stuffing in titles; they contradict the plain, technical tone.

Adapted from the SEO agents in wshobson/agents (`seo-meta-optimizer`, `seo-structure-architect`, `seo-cannibalization-detector`, `seo-content-refresher`, `seo-content-auditor`; MIT). We kept the measurable checks and dropped the persuasion tricks. The wording is ours.

## Images and video for every channel

Gallery images, social cards and video come from the design and video Teams, briefed with these sizes. Every asset travels with light and dark variants where it sits on a page, and media for review stays under 25 MB.
