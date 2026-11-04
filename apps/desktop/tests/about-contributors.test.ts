import assert from "node:assert/strict";
import test from "node:test";

import { fetchContributors, ContributorRateLimitError } from "../src/about/contributors.ts";

const contributor = (id: number, login = `user-${id}`) => ({
  id, login, type: "User", avatar_url: `https://avatars.githubusercontent.com/u/${id}?v=4`,
});
const firstPage = () => [contributor(1), ...Array.from({ length: 99 }, (_, index) => ({
  ...contributor(index + 2), type: "Bot",
}))];

test("contributors include later pages, exclude bots and unsafe avatars, and deduplicate accounts", async (context) => {
  const requests: string[] = [];
  context.mock.method(globalThis, "fetch", async (url: string) => {
    requests.push(String(url));
    return Response.json(requests.length === 1 ? firstPage() : [
      contributor(1), contributor(101),
      { ...contributor(102), avatar_url: "https://example.com/avatar.png" },
    ]);
  });

  const result = await fetchContributors(new AbortController().signal);
  assert.deepEqual(result.map((user) => user.id), [1, 101]);
  assert.equal(new URL(requests[1]).searchParams.get("page"), "2");
});

test("a failed later page rejects the whole refresh", async (context) => {
  let requests = 0;
  context.mock.method(globalThis, "fetch", async () => ++requests === 1
    ? Response.json(firstPage()) : new Response(null, { status: 503 }));

  await assert.rejects(fetchContributors(new AbortController().signal));
  assert.equal(requests, 2);
});

test("GitHub rate limits carry the server reset time", async (context) => {
  const reset = Math.ceil(Date.now() / 1000) + 3600;
  context.mock.method(globalThis, "fetch", async () => new Response(null, {
    status: 403, headers: { "x-ratelimit-remaining": "0", "x-ratelimit-reset": String(reset) },
  }));

  await assert.rejects(fetchContributors(new AbortController().signal), (error: unknown) => {
    assert.ok(error instanceof ContributorRateLimitError);
    assert.equal(error.retryAt, reset * 1000);
    return true;
  });
});
