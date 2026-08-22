import { request } from "./transport";
import type { Health } from "./types";

export const runtimeApi = {
  health: () => request<Health>("/health"),
};
