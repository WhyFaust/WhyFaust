# Updating the profile

The profile is generated from the public `WhyFaust` GitHub account. Its artwork uses the owner's approved mascot. Telegram is the owner's explicit link: <https://t.me/WhyFaust_tv>.

## Automatic refresh

The **Update profile** GitHub Actions workflow checks for changes every five minutes, at minutes 2, 7, 12, and so on. It also runs when the generator, mascot source, or workflow changes on `main`, and can be started with **Run workflow** in GitHub Actions.

It uses GitHub's built-in `GITHUB_TOKEN`. No personal access token, hosted statistics service, paid API, or additional Python package is required. The workflow can write this repository's contents; it does not publish through a separate external service.

The generator fetches every source before replacing any generated file. An unsuccessful API response, missing calendar count, or incomplete calendar fails the job and leaves the previously published profile intact. Concurrent refreshes are serialized. A concurrent human edit that makes the push non-fast-forward fails safely; the next scheduled run starts from the latest `main`.

When the semantic data has not changed, `updated_at` stays the same and the workflow creates no commit. The timestamp means **last change in the displayed data**, not the last scheduled check. Workflow run history records successful checks, including checks with no changes.

GitHub Actions scheduling is best effort. Runs can be delayed or dropped, GitHub data can lag, and image caching can postpone the visible refresh. This is automatically refreshed data, not a guarantee of immediate real-time delivery. GitHub can disable a public repository's scheduled workflows after 60 days without repository activity; its workflow page is the place to re-enable them.

## Data definitions

| Displayed data | Source and meaning |
| --- | --- |
| Name, biography, location, account creation, followers, following, public repository count | GitHub's public user REST endpoint. Empty biography/location remain empty in the snapshot. When the GitHub biography is empty, the renderer uses the owner's authored introduction from the existing profile README; a GitHub biography overrides that introduction when provided. Private email and other undisplayed fields are never copied to the snapshot. |
| Own projects | Public repositories owned by this account, excluding forks, this profile repository, and `github-profile-summary-cards` (another generated profile artifact). Archived projects still count in totals. |
| Stars and forks | Totals across those own projects. These are repository stars/forks, not personal skill or popularity scores. |
| Languages | Number of own projects whose GitHub-detected **primary language** matches each language. This is neither a code-byte percentage nor a proficiency score. Repositories without a detected primary language are omitted from the language distribution. |
| Featured projects | Actual pinned, owned, public, nonfork, nonarchived projects first; then the most recently pushed own active projects, up to four. Empty repositories with no push date are excluded. GitHub's automatic “Popular repositories” list is not treated as user-selected pins. |
| Contribution calendar | The public GitHub profile calendar for the latest 365 days, inclusive, with exact daily counts and GitHub intensity levels. Totals are contributions, not commits. It may include anonymous private contribution counts if the owner has enabled their public display; it never reveals private project names or content. |

The REST repository listing is paginated. A second ownership/visibility filter prevents publishing any private or other-owner repository data even if the runtime token can see more. Its count must match the public repository count returned by the user endpoint before project exclusions apply. An incomplete listing or a repository added/removed between requests fails safely; the next scheduled run tries the new state.

The public profile and contribution calendar HTML are read with Python's HTML parser. The calendar's date coverage and every count are checked. If GitHub changes that HTML, the refresh fails visibly instead of inventing zero activity; update the parser after inspecting the current public markup.

## Files and local commands

- `scripts/update_profile.py` fetches and validates the public sources.
- `scripts/render_profile.py` generates the README and theme-aware SVGs.
- `data/profile.json` contains only the normalized public data used in the profile.
- `assets/generated/` contains generated visuals.
- `assets/mascot.jpg` is the approved artwork used by the renderer. When replacing the portrait, update this JPEG.

Python 3.12 or newer is supported. From the repository root:

```bash
python3 scripts/update_profile.py
```

For a data-only refresh, use `python3 scripts/update_profile.py --fetch-only`. Without `GITHUB_TOKEN`, the public REST API's lower unauthenticated rate limit applies. The Actions job supplies its token automatically. Never commit a token to this repository.

Edit the renderer or source artwork to change the design; manual edits to generated files are replaced on the next successful run.

## Sources

- [GitHub profile README](https://docs.github.com/en/account-and-profile/how-tos/profile-customization/managing-your-profile-readme)
- [GitHub REST users](https://docs.github.com/en/rest/users/users#get-a-user)
- [GitHub REST user repositories](https://docs.github.com/en/rest/repos/repos#list-repositories-for-a-user)
- [Workflow schedule and its limitations](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule)
- [GitHub contribution visibility and delays](https://docs.github.com/en/account-and-profile/how-tos/contribution-settings/troubleshooting-missing-contributions)
- [GitHub image caching](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls)
- [Pinned checkout revision](https://github.com/actions/checkout/commit/d23441a48e516b6c34aea4fa41551a30e30af803)
