import assert from "node:assert/strict";
import test from "node:test";
import { defaultHiddenSourceLanguages, parsePlayerHistory, trackVisible, sourceLanguageVisible, subtitleVisible, subtitleSourceLanguage, livePartialSourceLanguage, historySpeakers, isAudioTrack } from "../src/subtitles/player-history.ts";
import type { LiveTranscription } from "../src/capture/types.ts";
import type { TranslationSettings } from "../src/settings/types.ts";
import type { Subtitle } from "../src/subtitles/types.ts";

test("history visibility is per track, preserves other tracks and does not hide the microphone", () => {
  const first = { id:"vrc:track-a", index:0, display_name:"Same name" };
  const second = { id:"vrc:track-b", index:1, display_name:"Same name" };
  assert.equal(trackVisible("speaker", first, [first.id]), false);
  assert.equal(trackVisible("speaker", second, [first.id]), true);
  assert.equal(trackVisible("microphone", first, [first.id]), true);
  assert.equal(trackVisible("speaker", null, ["other:unidentified"]), false);
  assert.equal(isAudioTrack(first), true);
  assert.equal(isAudioTrack({id:"qwen-session-1",index:0}), false);
});

test("hidden source languages match scripts and regions, leaving other languages and own messages visible", () => {
  const preferences = parsePlayerHistory(null);
  assert.deepEqual(preferences.hiddenSourceLanguages, []);
  for (const hidden of [["zh-Hans","en"], ["ja"]]) {
    assert.equal(sourceLanguageVisible("speaker", hidden[0], hidden), false);
    assert.equal(sourceLanguageVisible("speaker", "fr", hidden), true);
  }
  for (const language of ["zh", "zh-CN", "zh_Hant", "zh-TW", "en-US", "cmn", "zho", "Chinese", "Chinese (Traditional)", "English"]) {
    assert.equal(sourceLanguageVisible("speaker", language, ["zh-Hans","en"]), false);
  }
  assert.equal(sourceLanguageVisible(undefined, "zh", ["zh"]), false);
  assert.equal(sourceLanguageVisible("speaker", "ja", ["zh-Hans","en"]), true);
  assert.equal(sourceLanguageVisible("speaker", "en", []), true);
  assert.equal(sourceLanguageVisible("speaker", "en"), true);
  for (const language of [null, undefined, "", "auto", "und", "unknown"]) {
    assert.equal(sourceLanguageVisible("speaker", language, ["zh","en"]), true);
  }
  assert.equal(sourceLanguageVisible("microphone", "zh", ["zh"]), true);
  assert.equal(sourceLanguageVisible("chatbox", "zh", ["zh"]), true);
});

test("default hidden languages follow every other-party target and the active language session", () => {
  const target = (target_language: string) => ({target_language,profile_id:null,model:"",thinking_enabled:false});
  const translation = (codes: string[]): TranslationSettings => ({mode:"automatic",
    prompt:{system_prompt:"",context_enabled:false,include_speaker:false,include_microphone:false,include_chatbox:false,max_messages:0,max_chars:0},
    microphone_targets:[target("fr")], speaker_targets:codes.map(target)});
  const global = translation(["zh-Hans","en","zh-CN","zh-Hant"," en-US "," ","auto"]);
  assert.deepEqual(defaultHiddenSourceLanguages(global), ["zh","en"]);
  assert.deepEqual(defaultHiddenSourceLanguages(global,{kind:"global"}), ["zh","en"]);
  assert.deepEqual(defaultHiddenSourceLanguages(global,{kind:"override",translation:translation(["ja","de"]),speaker_recognition_languages:[],microphone_recognition_languages:[],osc_translation_strategy:"all_languages"}), ["ja","de"]);
  assert.deepEqual(defaultHiddenSourceLanguages(global,{kind:"preset",preset_id:"other-languages",preset_name:"Other languages",translation:translation(["ko","ja"]),speaker_recognition_languages:[],microphone_recognition_languages:[],osc_translation_strategy:"all_languages"}), ["ko","ja"]);
  assert.deepEqual(defaultHiddenSourceLanguages(), []);
  assert.deepEqual(defaultHiddenSourceLanguages(translation([])), []);
});

test("display preferences persist without changing the audio selection; malformed storage falls back", () => {
  const preferences = {hiddenTracks:["vrc:track-b"], hiddenSourceLanguages:["en","ja"]};
  assert.deepEqual(parsePlayerHistory(JSON.stringify(preferences)), preferences);
  assert.deepEqual(parsePlayerHistory("broken"), {hiddenTracks:[],hiddenSourceLanguages:[]});
  assert.deepEqual(parsePlayerHistory('{"hiddenTracks":["a"],"hiddenSourceLanguages":null}'), {hiddenTracks:["a"],hiddenSourceLanguages:[]});
  assert.deepEqual(parsePlayerHistory('{"hiddenTracks":["a","a",10],"hiddenSourceLanguages":["zh_Hant","zh","en-US",false]}'), {hiddenTracks:["a"], hiddenSourceLanguages:["zh","en"]});
  assert.deepEqual(parsePlayerHistory('{"hiddenSourceLanguages":[]}'), {hiddenTracks:[], hiddenSourceLanguages:[]});
});

test("source-language filtering never uses a translation target as the original language", () => {
  const row: Subtitle = {id:1,source:"speaker",text:"original",language:"ja",created_at:"",started_at:null,ended_at:null,
    translations:[{text:"中文译文",source_language:"ja",target_language:"zh-Hans",provider:"local",model:null,created_at:""}]};
  assert.equal(sourceLanguageVisible(row.source, subtitleSourceLanguage(row), ["zh"]), true);
  assert.equal(sourceLanguageVisible(row.source, subtitleSourceLanguage(row), ["ja"]), false);
  row.language = null;
  assert.equal(subtitleSourceLanguage(row), "ja");
  row.translations[0].source_language = "zh-CN";
  assert.equal(sourceLanguageVisible(row.source, subtitleSourceLanguage(row), ["zh"]), false);
  row.translations.push({...row.translations[0],source_language:"ja"});
  assert.equal(subtitleSourceLanguage(row), null);
  row.translations = [];
  assert.equal(subtitleSourceLanguage(row), null);
});

test("backend detected_language fallback hides records without provider metadata", () => {
  for (const text of ["这是一段用于测试的中文", "今天我们准备检查中文过滤功能", "這是繁體中文的測試", "你好", "谢谢"]) {
    const row: Subtitle = {id:1, source:"speaker", text, language:null, detected_language:"zh", created_at:"", started_at:null, ended_at:null,
      translations:[{text:"中文翻译", source_language:null, target_language:"zh-Hans", provider:"local", model:null, created_at:""}]};
    assert.equal(subtitleSourceLanguage(row), "zh", text);
    assert.equal(sourceLanguageVisible(row.source, subtitleSourceLanguage(row), ["zh-Hans"]), false, text);
    assert.equal(sourceLanguageVisible("microphone", subtitleSourceLanguage(row), ["zh"]), true);
    assert.equal(sourceLanguageVisible(row.source, subtitleSourceLanguage(row), []), true);
  }
});

test("detected_language fallback preserves other originals and ignores translation text", () => {
  const examples: [string, string | null][] = [
    ["同時通訳ってすごいですね", "ja"],
    ["오늘 친구들과 이야기하고 있어요", "ko"],
    ["Thank you very much", "en"],
    ["東京", null],
  ];
  for (const [text, expected] of examples) {
    const row: Subtitle = {id:1, source:"speaker", text, language:null, detected_language:expected, created_at:"", started_at:null, ended_at:null,
      translations:[{text:"今天我们准备检查中文过滤功能", source_language:null, target_language:"zh-Hans", provider:"local", model:null, created_at:""}]};
    assert.equal(subtitleSourceLanguage(row), expected, text);
    assert.equal(sourceLanguageVisible(row.source, subtitleSourceLanguage(row), ["zh"]), true, text);
    if (expected) assert.equal(sourceLanguageVisible(row.source, subtitleSourceLanguage(row), ["zh", expected]), false, text);
    row.language = "yue-Hant";
    assert.equal(subtitleSourceLanguage(row), "yue", "provider language always wins over detected_language");
  }
});

test("live filtering uses detected_language until provider language arrives", () => {
  const partial: LiveTranscription = {type:"partial", source:"speaker", utterance_id:"qwen-preview", language:null,
    text:"…", translation:"中文译文", target_language:"zh-Hans"};
  assert.equal(livePartialSourceLanguage(partial), null, "translation-only updates cannot identify the original language");
  partial.text = "这是一段用于测试的中文";
  partial.detected_language = "zh";
  assert.equal(livePartialSourceLanguage(partial), "zh");
  assert.equal(sourceLanguageVisible(partial.source, livePartialSourceLanguage(partial), ["zh"]), false);
  partial.detected_language = "ja";
  assert.equal(livePartialSourceLanguage(partial), "ja");
  assert.equal(sourceLanguageVisible(partial.source, livePartialSourceLanguage(partial), ["zh"]), true);
  partial.language = "ko";
  assert.equal(livePartialSourceLanguage(partial), "ko", "late provider metadata takes precedence over detected_language");
});

test("history roster preserves distinct tracks with the same name and discovers unidentified records", () => {
  const row = (speaker?: Subtitle["speaker"], source: Subtitle["source"] = "speaker"): Subtitle => ({speaker,source,translations:[], id:null, text:"record", language:null, started_at:null, ended_at:null, created_at:"2026-10-25T00:00:00Z"});
  const a={id:"vrc:a",index:0,display_name:"Same"}, b={id:"vrc:b",index:1,display_name:"Same"};
  assert.deepEqual(historySpeakers([row(a),row(b),row(a),row(undefined),row(a,"microphone")]).map((speaker)=>speaker.id), ["vrc:a","vrc:b","other:unidentified"]);
});

test("remote Chatbox shares other-player visibility and discovers text-only players", () => {
  const speaker = { id: "vrc:text-only", index: 4 };
  const row = { id: 41, source: "vrchat_chatbox", speaker, translations: [], text: "Bonjour tout le monde",
    language: "fr", started_at: null, ended_at: null, created_at: "" } as Subtitle;
  assert.equal(subtitleVisible(row, [speaker.id]), false);
  assert.equal(subtitleVisible(row, [], ["fr"]), false);
  assert.equal(subtitleVisible(row, [], ["en"]), true);
  assert.deepEqual(historySpeakers([row]), [speaker]);
  assert.equal(subtitleVisible({ ...row, source: "chatbox" }, [speaker.id], ["fr"]), true);
});
