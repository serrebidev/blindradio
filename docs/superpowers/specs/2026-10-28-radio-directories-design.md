# Additional radio directories

## Intent

Expand BlindRadio's station discovery using the directories and networks listed at
<https://fmhy.net/audio#radio-streaming>. The user selected both a new live directory
and curated networks, then explicitly added SHOUTcast to the scope.
After approving the design, the user also requested Internet-Radio.com.

## Approaches

1. Add Icecast alone: a public machine-readable directory, but no expansion of curated networks.
2. Expand curated networks alone: the smallest data change, but no new directory.
3. Add both: the user's selection. Also add SHOUTcast, as subsequently requested.

Use the existing Sources dispatcher, Station model, browse tree, search merger,
network data, playlist resolver, favorites and collections. No separate interface.

## Behavior

- Add Icecast and SHOUTcast roots after iHeartRadio and before Networks.
- Icecast offers All Stations and By Genre. Read <https://dir.xiph.org/yp.xml>,
  omit blank names, invalid/non-HTTP URLs and non-audio entries, deduplicate by URL,
  and sort names alphabetically. Paginate station lists in groups of 100 using
  the existing Load More node. Genre folders use the feed's genre labels.
- Cache Icecast's parsed feed in memory for 15 minutes, shared between browsing
  and search. Concurrent callers share one fetch. Limit the response body to 16 MiB.
  Do not write another persistent cache. On failure, use existing directory errors
  and search's existing eight-second timeout rather than blocking other sources.
- SHOUTcast offers Top Stations and By Genre. Obtain genre names from the public
  directory page, and use the same JSON POST endpoints as its website:
  /Home/Top, /Home/BrowseByGenre and /Search/UpdateSearch. Keep provider order and
  paginate station lists in groups of 100. Use its /sbin/tunein-station.pls?id=
  links with the existing playlist resolver. No API key or account settings.
- Include both sources in combined name searches, retaining existing URL
  deduplication and directory failure isolation. Icecast supports local genre
  matching too. Country and language filters remain Radio Browser features;
  keep filter-only searches on Radio Browser to preserve current behavior.
- Add Internet-Radio.com as an Internet-Radio root with Featured Stations and
  By Genre, plus combined name search. Parse its public HTML using a real HTML
  parser, follow its genre/search pagination through Load More, and use the
  published playlist links with the existing resolver. Accept only same-site
  navigation paths and HTTP stream targets. Website changes are a maintenance risk.
- Add Radio Paradise, NTS and WFMU under Networks, using only official published
  stream links that pass a live connection check. Include verified current
  channels; exact channel count is left to implementation based on verification.
- Preserve existing station IDs and serialization. Add explicit Icecast and
  SHOUTcast source labels and stable IDs for the new directories.
- Update the in-app guide to describe the new roots and filtering limits.

## Constraints and exclusions

Native browse and search controls already provide keyboard and screen-reader
access. No new widgets, shortcuts or continuous announcements are needed.
Existing Loading, Load More and error behavior applies to the new roots.

Reuse installed dependencies and existing helpers. A direct quick-xml dependency
with version "*" is appropriate for the XML feed; it already exists in Cargo.lock.
Do not build a custom XML parser. Do not change audio playback, updater behavior,
saved libraries, release configuration or existing station URLs.

No automatic website scraping beyond SHOUTcast's public genre list, no borrowed
API credentials, no new SomaFM API dependency, and no release or install in this task.

## Evidence and risks

On 2026-10-28, Icecast returned approximately 8.5 MB of XML, 30,574 entries, and
10,465 HTTP stream URLs. Empty URLs are common; listing is not proof of playback.
SHOUTcast's public JSON POST endpoints and two station PLS links returned HTTP 200.
These endpoints are undocumented and can change independently of BlindRadio.

SomaFM already has all 51 channels currently listed in its channel feed. Its
official API page says third-party API access is unavailable, so leave it alone.

Primary references:

- <https://dir.xiph.org/>
- <https://directory.shoutcast.com/js/common.js?_=V22>
- <https://directory.shoutcast.com/js/station-list.js>
- <https://radioparadise.com/listen/stream-links>
- <https://ntslive.freshdesk.com/support/solutions/articles/77000587257-tunein>
- <https://wfmu.org/ssaudionet.shtml/index.shtml>
- <https://somafm.com/linktous/api.html>

## Verification

Use local HTTP fixtures for parsing, malformed responses, URL validation,
pagination, cache reuse, browse dispatch, search merging and failure isolation.
Check new source JSON round-trips and preservation of old saved station formats.
Update network and browse-root expectations. Run workspace tests, formatting,
build and clippy. Check both live directories and each curated stream connection.
Report any manual NVDA or audible playback checks that could not be performed.
