export type ApiErrorParams = Record<string, unknown>;

export interface ApiErrorPayload {
  code?: unknown;
  params?: unknown;
  detail?: unknown;
}

function isRecord(value: unknown): value is ApiErrorParams {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export class ApiError extends Error {
  readonly code: string;
  readonly params: ApiErrorParams;
  readonly detail: string;
  readonly status: number;

  constructor(options: {
    code: string;
    params?: ApiErrorParams;
    detail: string;
    status: number;
  }) {
    super(options.detail);
    this.name = "ApiError";
    this.code = options.code;
    this.params = options.params ?? {};
    this.detail = options.detail;
    this.status = options.status;
  }
}

export async function apiErrorFromResponse(response: Response): Promise<ApiError> {
  const fallbackDetail = `${response.status} ${response.statusText}`.trim();
  const body = (await response.json().catch(() => null)) as ApiErrorPayload | null;
  return new ApiError({
    code: typeof body?.code === "string" && body.code
      ? body.code
      : "http.unexpected_response",
    params: isRecord(body?.params) ? body.params : { status: response.status },
    detail: typeof body?.detail === "string" && body.detail
      ? body.detail
      : fallbackDetail,
    status: response.status,
  });
}
