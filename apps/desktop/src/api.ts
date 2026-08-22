// Compatibility façade. New code should import the API owned by its domain.
import { ankiApi } from "./anki/api";
import { captureApi } from "./capture/api";
import { chatboxApi } from "./chatbox/api";
import { conversationsApi } from "./conversations/api";
import { runtimeApi } from "./core-client/runtime-api";
import { dictionaryApi } from "./dictionary/api";
import { integrationsApi } from "./integrations/api";
import { learningApi } from "./learning/api";
import { providersApi } from "./providers/api";
import { glossaryApi } from "./settings/glossary/api";
import { settingsApi } from "./settings/api";
import { translationSettingsApi } from "./settings/translation-api";
import { storageApi } from "./storage/api";
import { subtitlesApi } from "./subtitles/api";

export {
  coreStartup,
  initializeCoreApi,
  retryCore,
} from "./core-client/startup-api";
export type { CoreStartup } from "./core-client/startup-api";
export { coreWebSocketUrl } from "./core-client/transport";

export const coreApi = {
  health: runtimeApi.health,
  subtitles: subtitlesApi.subtitles,
  conversations: conversationsApi.conversations,
  createConversation: conversationsApi.createConversation,
  updateConversation: conversationsApi.updateConversation,
  deleteConversation: conversationsApi.deleteConversation,
  conversationSubtitles: conversationsApi.conversationSubtitles,
  storageStats: storageApi.storageStats,
  clearSubtitleHistory: storageApi.clearSubtitleHistory,
  deleteSubtitleRange: storageApi.deleteSubtitleRange,
  devices: captureApi.devices,
  settings: settingsApi.settings,
  saveSettings: settingsApi.saveSettings,
  externalApiTokenStatus: integrationsApi.externalApiTokenStatus,
  externalApiRuntimeStatus: integrationsApi.externalApiRuntimeStatus,
  saveExternalApiToken: integrationsApi.saveExternalApiToken,
  deleteExternalApiToken: integrationsApi.deleteExternalApiToken,
  vrcxTokenStatus: integrationsApi.vrcxTokenStatus,
  vrcxRuntimeStatus: integrationsApi.vrcxRuntimeStatus,
  saveVrcxToken: integrationsApi.saveVrcxToken,
  deleteVrcxToken: integrationsApi.deleteVrcxToken,
  testVrcx: integrationsApi.testVrcx,
  start: captureApi.start,
  stop: captureApi.stop,
  startMicrophoneTest: captureApi.startMicrophoneTest,
  stopMicrophoneTest: captureApi.stopMicrophoneTest,
  testOsc: integrationsApi.testOsc,
  previewChatbox: chatboxApi.previewChatbox,
  sendChatbox: chatboxApi.sendChatbox,
  asrCapabilities: providersApi.asrCapabilities,
  asrModels: providersApi.asrModels,
  apiProfiles: providersApi.apiProfiles,
  providers: providersApi.providers,
  createApiProfile: providersApi.createApiProfile,
  updateApiProfile: providersApi.updateApiProfile,
  deleteApiProfile: providersApi.deleteApiProfile,
  saveApiProfileCredential: providersApi.saveApiProfileCredential,
  deleteApiProfileCredential: providersApi.deleteApiProfileCredential,
  activateAsrProfile: providersApi.activateAsrProfile,
  testApiProfile: providersApi.testApiProfile,
  apiProfileModels: providersApi.apiProfileModels,
  recognitionServiceModels: providersApi.recognitionServiceModels,
  translateSubtitle: subtitlesApi.translateSubtitle,
  previewTranslation: subtitlesApi.previewTranslation,
  previewTranslationPrompt: translationSettingsApi.previewTranslationPrompt,
  glossaryStatuses: glossaryApi.glossaryStatuses,
  refreshGlossary: glossaryApi.refreshGlossary,
  downloadAsrModel: providersApi.downloadAsrModel,
  deleteAsrModel: providersApi.deleteAsrModel,
  learningItems: learningApi.learningItems,
  learningCaptureKeys: learningApi.learningCaptureKeys,
  createLearningItem: learningApi.createLearningItem,
  updateLearningItem: learningApi.updateLearningItem,
  archiveLearningItem: learningApi.archiveLearningItem,
  restoreLearningItem: learningApi.restoreLearningItem,
  deleteLearningItem: learningApi.deleteLearningItem,
  analyzeLearningItem: learningApi.analyzeLearningItem,
  querySelection: learningApi.querySelection,
  generateLearningDraft: learningApi.generateLearningDraft,
  exportLearningItem: learningApi.exportLearningItem,
  lookup: dictionaryApi.lookup,
  dictionaries: dictionaryApi.dictionaries,
  importDictionary: dictionaryApi.importDictionary,
  deleteDictionary: dictionaryApi.deleteDictionary,
  ankiStatus: ankiApi.ankiStatus,
  createCard: ankiApi.createCard,
};
