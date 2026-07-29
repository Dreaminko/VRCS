export function shouldShowVrchatNotRunningWarning(
  error: unknown,
  vrchatOnly: boolean,
): boolean {
  return vrchatOnly
    && typeof error === "object"
    && error !== null
    && "code" in error
    && error.code === "audio.vrchat_not_running";
}
