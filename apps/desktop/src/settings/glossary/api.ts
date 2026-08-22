import { request } from "../../core-client/transport";
import type { GlossarySourceStatus } from "../types";

export const glossaryApi = {
  glossaryStatuses: () => request<GlossarySourceStatus[]>("/api/glossaries/status"),
  refreshGlossary: (id: string) => request<unknown>(
    `/api/glossaries/${encodeURIComponent(id)}/refresh`,
    { method: "POST" },
  ),
};
