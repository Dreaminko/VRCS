export const API_PROFILES_CHANGED_EVENT = "vrcs:api-profiles-changed";

export function notifyApiProfilesChanged(): void {
  window.dispatchEvent(new Event(API_PROFILES_CHANGED_EVENT));
}
