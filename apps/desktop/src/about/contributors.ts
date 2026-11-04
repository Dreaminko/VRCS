export interface Contributor {
  id: number;
  login: string;
  avatar_url: string;
}

export interface ContributorCache {
  checkedAt: number;
  contributors: Contributor[];
}

export const CONTRIBUTOR_REFRESH_INTERVAL = 30 * 60_000;
export const CONTRIBUTOR_REFRESH_COOLDOWN = 60_000;
const CACHE_KEY = "vrcs.about.contributors.v1";
const API_URL = "https://api.github.com/repos/Dreaminko/VRCS/contributors";

export class ContributorRateLimitError extends Error {
  retryAt: number;

  constructor(retryAt: number) {
    super("GitHub contributor requests are rate limited");
    this.retryAt = retryAt;
  }
}

function isContributor(value: unknown): value is Contributor {
  if (!value || typeof value !== "object") return false;
  const user = value as Contributor;
  return Number.isSafeInteger(user.id) && user.id > 0
    && typeof user.login === "string" && /^[a-z\d](?:[a-z\d-]{0,37}[a-z\d])?$/i.test(user.login)
    && typeof user.avatar_url === "string"
    && /^https:\/\/avatars\.githubusercontent\.com\//.test(user.avatar_url);
}

export function loadContributorCache(): ContributorCache | null {
  try {
    const cache = JSON.parse(localStorage.getItem(CACHE_KEY) ?? "null") as ContributorCache | null;
    return cache && Number.isFinite(cache.checkedAt) && cache.checkedAt > 0
      && cache.checkedAt <= Date.now() && Array.isArray(cache.contributors)
      && cache.contributors.every(isContributor) ? cache : null;
  } catch {
    return null;
  }
}

export function saveContributorCache(cache: ContributorCache): void {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(cache));
  } catch {
    // The page remains usable when local storage is unavailable.
  }
}

export async function fetchContributors(signal: AbortSignal): Promise<Contributor[]> {
  const contributors = new Map<number, Contributor>();
  for (let page = 1; ; page++) {
    const response = await fetch(`${API_URL}?per_page=100&page=${page}`, {
      headers: { Accept: "application/vnd.github+json" },
      signal: AbortSignal.any([signal, AbortSignal.timeout(10_000)]),
    });
    if (response.status === 429 || (response.status === 403
      && (response.headers.get("x-ratelimit-remaining") === "0" || response.headers.has("retry-after")))) {
      throw new ContributorRateLimitError(Math.max(
        Date.now() + CONTRIBUTOR_REFRESH_COOLDOWN,
        (Number(response.headers.get("x-ratelimit-reset")) || 0) * 1000,
        Date.now() + (Number(response.headers.get("retry-after")) || 0) * 1000,
      ));
    }
    if (!response.ok) throw new Error(`GitHub contributors: ${response.status}`);
    const users: unknown = response.status === 204 ? [] : await response.json();
    if (!Array.isArray(users)) throw new Error("Invalid GitHub contributors response");
    for (const user of users) {
      if (user?.type === "User" && isContributor(user) && !contributors.has(user.id)) {
        contributors.set(user.id, { id: user.id, login: user.login, avatar_url: user.avatar_url });
      }
    }
    if (users.length < 100) return [...contributors.values()];
  }
}
