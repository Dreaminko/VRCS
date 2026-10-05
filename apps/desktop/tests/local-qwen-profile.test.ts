import assert from "node:assert/strict";
import test from "node:test";

import { createApiProfileDraft } from "../src/api-profile-draft.ts";
import type { ProviderDefinition } from "../src/providers/types.ts";

test("local Qwen ASR profile starts with a longer transcription timeout and no key", () => {
  const localQwen: ProviderDefinition = {
    id: "qwen_local",
    display_name: "Local Qwen ASR",
    category: "local_service",
    connection: {
      base_url: { mode: "editable", default: "http://127.0.0.1:8000/v1" },
      auth_modes: ["none"],
      default_auth_mode: "none",
      fields: [],
    },
    services: [{
      id: "qwen_local_transcription",
      display_name: "Local Qwen ASR",
      capabilities: ["speech_to_text"],
      adapter: "open_ai_audio_transcriptions",
      recognition_transport: "segmented_upload",
      partial_results: false,
      models: ["Qwen/Qwen3-ASR-0.6B"],
      model_listing: false,
      supports_context: false,
    }],
    support_levels: { asr: "protocol_compatible", translation: null },
    capabilities: {
      supports_streaming: false,
      supports_model_listing: false,
      requires_api_key: false,
      is_local: true,
      supports_context: false,
      supports_translation: false,
      supports_asr: true,
      supports_text_generation: false,
      supports_custom_translation_language: false,
      supported_languages: [],
    },
  };

  const draft = createApiProfileDraft([localQwen], localQwen.id);
  assert.deepEqual(draft.enabled_capabilities, ["speech_to_text"]);
  assert.equal(draft.base_url, "http://127.0.0.1:8000/v1");
  assert.equal(draft.auth_mode, "none");
  assert.equal(draft.timeout_ms, 30_000);
});
