import { useEffect, useSyncExternalStore } from "react";

import { providersApi } from "./api";
import type { ApiProfileView, ProviderDefinition } from "./types";

interface ApiProfileCatalog {
  profiles: ApiProfileView[];
  providerDefinitions: ProviderDefinition[];
  loading: boolean;
  error: unknown;
}

let catalog: ApiProfileCatalog = { profiles: [], providerDefinitions: [], loading: true, error: null };
let loaded = false;
let requestVersion = 0;
let pending: Promise<void> | undefined;
let definitions: Promise<ProviderDefinition[]> | undefined;
const listeners = new Set<() => void>();

function publish(patch: Partial<ApiProfileCatalog>) {
  catalog = { ...catalog, ...patch };
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function refreshApiProfileCatalog(force = false): Promise<void> {
  if (!force && pending) return pending;
  if (!force && loaded && !catalog.error) return Promise.resolve();
  const version = ++requestVersion;
  definitions ??= providersApi.providers().then((response) => response.providers).catch((reason) => {
    definitions = undefined;
    throw reason;
  });
  publish({ loading: !loaded, error: null });
  pending = Promise.all([providersApi.apiProfiles(), definitions]).then(
    ([response, providerDefinitions]) => {
      if (version !== requestVersion) return;
      loaded = true;
      publish({ profiles: response.profiles, providerDefinitions, loading: false, error: null });
    },
    (error: unknown) => {
      if (version === requestVersion) publish({ loading: false, error });
      throw error;
    },
  ).finally(() => {
    if (version === requestVersion) pending = undefined;
  });
  return pending;
}

export function useApiProfileCatalog(enabled = true): ApiProfileCatalog {
  const snapshot = useSyncExternalStore(subscribe, () => catalog);
  useEffect(() => {
    if (enabled) void refreshApiProfileCatalog().catch(() => undefined);
  }, [enabled]);
  return snapshot;
}
