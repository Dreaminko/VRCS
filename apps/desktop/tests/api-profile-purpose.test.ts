import assert from "node:assert/strict";
import test from "node:test";

import {
  apiProfilePurpose,
  supportsLlmModels,
  supportsRecognition,
  supportsTranslation,
} from "../src/api-profile-purpose.ts";
import type { ApiProfile } from "../src/types.ts";

const profile = (input: Partial<ApiProfile>): ApiProfile => ({
  id: "profile",
  name: "Profile",
  provider: "openai",
  ...input,
});

test("legacy API profiles infer their existing purpose", () => {
  const officialOpenAi = profile({});
  const compatibleOpenAi = profile({ base_url: "https://api.deepseek.com/v1" });
  const deepL = profile({ provider: "deepl" });

  assert.equal(apiProfilePurpose(officialOpenAi), "shared");
  assert.equal(apiProfilePurpose(compatibleOpenAi), "llm");
  assert.equal(apiProfilePurpose(deepL), "llm");
});

test("explicit API purposes isolate ASR and translation", () => {
  const asrOnly = profile({ purpose: "asr" });
  const llmOnly = profile({ purpose: "llm" });
  const compatible = profile({ purpose: "llm", base_url: "https://api.deepseek.com/v1" });

  assert.equal(supportsRecognition(asrOnly), true);
  assert.equal(supportsTranslation(asrOnly), false);
  assert.equal(supportsLlmModels(asrOnly), false);
  assert.equal(supportsRecognition(llmOnly), false);
  assert.equal(supportsTranslation(llmOnly), true);
  assert.equal(supportsLlmModels(compatible), true);
});
