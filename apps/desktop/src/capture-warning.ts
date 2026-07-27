const VRCHAT_NOT_RUNNING_ERROR = "未发现正在运行的 VRChat";

export function shouldShowVrchatNotRunningWarning(
  message: string,
  vrchatOnly: boolean,
): boolean {
  return vrchatOnly && message.includes(VRCHAT_NOT_RUNNING_ERROR);
}
