# VRCS Nightly 发布方案

北京时间每天 00:17 结算前一个自然日。`main` 当天有提交推送或合并时，构建该日最后一次变更对应的 Windows x64 安装包，发布一份带 Tauri 更新签名的 GitHub Pre-release。用户在「设置 → 系统 → 软件更新」勾选「接收 Nightly 更新」后，应用在检查到可用版本时提醒，由用户确认安装。

已实现定时发布和桌面端订阅。上线时先发布首份 Nightly，再发布含订阅开关的正式版。

## 1. 每日变更检测

- 新增 `.github/workflows/nightly.yml`，使用 `17 16 * * *`（UTC），定时和手动触发均从 `main` 执行。用 GitHub Actions run 的 `created_at` 换算北京时间，取前一自然日作为构建日期 `D`。排队、构建跨午夜或重新运行同一个 run，日期仍取原 run 的值。`workflow_dispatch` 接收已结束的北京时间日期，用于补跑延迟、漏跑或失败的窗口。[GitHub Actions 定时触发](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule)
- 查询 GitHub 仓库活动 API 的 `ref=refs/heads/main`，按时间降序分页，在 `[D 00:00, D+1 00:00)` 取最后一条改变 `main` 提交的 `push`、`pr_merge`、`merge_queue_merge` 或 `force_push` 记录。以活动的 `timestamp` 判定日期，以 `after` 固定构建 SHA。文档提交也算仓库更新。开发分支、标签和 Release 发布不属于 `main` 变更。[GitHub 仓库活动 API](https://docs.github.com/en/rest/repos/repos#list-repository-activities)
- 无匹配记录时直接跳过构建和发布。活动查询失败、SHA 无法检出或 SHA 已脱离当前 `main` 历史时，标记任务失败并保留已发布清单。检测后的检出、CI、安装包和标签均使用选中的完整 SHA。
- 同一日期使用唯一标签 `nightly-YYYYMMDD`。定时和手动发布共用并发组，设置 `cancel-in-progress: false`。已发布日期补跑时复用其发布资产。标签已经存在时校验指向，一旦与候选 SHA 不符便停止发布。

## 2. 版本号与 Windows 构建

已发布的正式版为 `S`，目标提交中 `tauri.conf.json` 的基础版本为 `B`。Nightly 的核心版本取 `max(B, nextPatch(S))`，再附加 `-nightly.YYYYMMDD`。`nextPatch` 只增加补丁号。`B` 与 `S` 均取正式 SemVer，`S` 以正式更新源中的版本为准。发布前确认目标提交包含 `S` 的正式标签提交，补跑旧日期时同样适用。

例如，正式版和仓库基础版本均为 `0.3.8` 时，升级顺序为 `0.3.8` → `0.3.9-nightly.20261203` → `0.3.9-nightly.20261204` → `0.3.9`。如果仓库已将基础版本升到 `0.4.0`，该提交使用 `0.4.0-nightly.YYYYMMDD`。标签以 `nightly-` 开头，与正式发布工作流的数字标签匹配规则隔离。[正式发布工作流](../../.github/workflows/release.yml) · [Tauri Updater](https://v2.tauri.app/plugin/updater/)

1. 在 `find-release-ci.mjs` 增加显式目标 SHA，用于复用同一提交的完整成功 CI。复用条件不满足时调用现有 `ci.yml`，传入选定 SHA 并在每个检出点使用它。这样即使执行时 `main` 已前进，验证和打包仍针对同一份源码。[CI 复用检查](../../scripts/find-release-ci.mjs) · [CI 工作流](../../.github/workflows/ci.yml)
2. 复用 `windows-2022`、锁定的工具链和 Windows x64 NSIS 打包。`build-release.ps1` 区分渠道，先校验仓库内 Tauri、桌面 Cargo、Core Cargo 和桌面 npm 的基础版本一致。Nightly 把有效版本写入现有生成的 Tauri Release 配置，Git 中的版本文件和锁文件保留基础版本。正式包继续校验标签与基础版本一致。[打包脚本](../../scripts/build-release.ps1) · [Tauri Release 配置](../../apps/desktop/src-tauri/tauri.release.conf.json)
3. `app_build_info` 以 `app.package_info().version` 为准，使「关于」、更新设置和 Tauri Updater 使用同一版本。启动日志和诊断报告也采用应用包版本。现有包标识 `app.vrcs.desktop`、安装位置和用户数据目录用于两种发布渠道。[原生更新检查](../../apps/desktop/src-tauri/src/app_updates.rs) · [安装目录迁移钩子](../../apps/desktop/src-tauri/installer-hooks.nsh)
4. 使用现有 Tauri 更新密钥签名。CI 通过后构建，运行现有 `--release-self-test`，生成 `VRCS-<版本>-windows-x64.exe`、`.sig`、`.sha256` 和 `nightly.json`。复用 `write-release-metadata.ps1` 的安装包与校验和检查，Nightly 清单的安装包 URL 使用 `nightly-YYYYMMDD` 标签，并提供 `windows-x86_64-standard` 目标。[清单生成脚本](../../scripts/write-release-metadata.ps1)

## 3. 发布及更新地址

| 客户端渠道 | 清单地址 | 安装包位置 |
| --- | --- | --- |
| 正式版 | `https://github.com/Dreaminko/VRCS/releases/latest/download/latest.json` | 正式版本标签的 Release，保留旧 CUDA 客户端所需的目标条目 |
| Nightly | `https://raw.githubusercontent.com/Dreaminko/VRCS/nightly-updates/nightly.json` | `nightly-YYYYMMDD` 标签的 Pre-release |

发布作业先创建绑定目标 SHA 的 Nightly draft，上传并核对四份资产，再以 `prerelease`、`latest=false` 发布。发布页注明构建日期、完整 SHA 与 Actions run 链接。最后将同一份 `nightly.json` 写入专用的 `nightly-updates` 分支，供客户端读取。清单使用 Tauri 的 `version`、`notes`、`pub_date`、`platforms` 字段，并带上构建日期与源码 SHA 供发布作业核对。[GitHub CLI 发布选项](https://cli.github.com/manual/gh_release_create) · [正式 Latest 的选取规则](https://docs.github.com/en/rest/releases/releases#get-the-latest-release)

构建、签名或上传失败时，客户端继续读上一次成功的清单。若版本已发布而清单更新失败，同日补跑从该 Pre-release 取回清单，核对版本、标签、SHA 和资产后更新。分支首次发布时创建，后续采用 fast-forward 更新。已公开的清单只接受日期及 SemVer 都更高的新候选，已经指向同一版本时视为完成。

仅发布作业拥有 `contents: write`，活动检测与 CI 复用检查需要 `contents: read` 和 `actions: read`。签名密钥仅传给构建签名步骤。首份有效 Nightly 发布后，再发布包含订阅开关的正式客户端。

GitHub 为历史提交创建引用时可能要求额外的 Workflows 写权限。可配置仅限本仓库、具备 Contents 和 Workflows 写权限的 `NIGHTLY_REF_TOKEN`，发布作业只在创建引用时使用它。[GitHub 引用权限](https://docs.github.com/en/rest/git/refs#create-a-reference)

## 4. 用户订阅与版本选择

在「自动检查更新」旁添加「接收 Nightly 更新」，将 `receiveNightlyUpdates` 保存在现有 `preferences.json`。旧用户、首次安装和缺少该键的配置默认关闭。开关说明标明 Nightly 可能不稳定，两种版本共用本机数据。设置、版本及更新提示覆盖简体中文、繁体中文、英文和日文。[更新偏好](../../apps/desktop/src/updates/update-preferences.ts) · [软件更新设置](../../apps/desktop/src/updates/SoftwareUpdateSettings.tsx)

| 订阅状态 | 检查与选择 |
| --- | --- |
| 关闭 | 只读取正式清单。当前安装的是 Nightly 时，等待 SemVer 更高的正式版。 |
| 开启 | 分别检查正式与 Nightly 清单。候选需符合对应渠道且高于当前安装版本，从中选择 SemVer 最高的一项。同一核心版本的正式版优先于 Nightly。 |

- 复用引导完成后的启动检查和运行中每 24 小时检查。保存开关后，自动检查已开启时立即按新设置检查，自动检查已关闭时通过「立即检查」读取新设置。应用内提醒标出渠道和版本号，沿用已有的签名校验、下载进度和安装确认。[检查与安装状态](../../apps/desktop/src/updates/useAppUpdater.ts) · [应用内更新提示](../../apps/desktop/src/updates/UpdateNotice.tsx)
- 原生端从共享偏好存储读取已保存的订阅状态，使用两个固定地址分别调用 Tauri Updater 并按 SemVer 比较候选。将渠道随更新结果返回给界面，平台变体仍为 `standard`。某个地址失败时仍可提示另一地址的有效更新。手动检查遇到渠道失败且无候选时报告故障渠道。所有已选渠道均检查成功且无候选时显示「已是最新版本」。[Tauri Updater](https://v2.tauri.app/plugin/updater/)
- 通过原生命令保存订阅，与检查及安装共用 `UpdateState` 的互斥状态。检查、下载、安装或保存中禁用开关。保存成功后清除原生待安装项和应用内旧提示。保存失败时恢复原订阅状态并显示错误。每次检查开始也清除旧的待安装项，安装时核对界面确认的版本与原生待安装项一致。取消订阅后，先前提示的 Nightly 随候选一同撤下。[原生更新状态](../../apps/desktop/src-tauri/src/app_updates.rs)

## 5. 验收场景

| 场景 | 通过条件 |
| --- | --- |
| 当天无 `main` 变更，或当天仅有开发分支活动 | 检测完成，跳过 Windows 构建、Pre-release 和清单更新。 |
| 当天有多次推送、旧提交在当日才推送、或推送恰好跨北京时间午夜 | 以服务端活动时间归日，当日最后一个 `after` 为唯一 Nightly 的构建 SHA。午夜时刻归入新一天。 |
| 定时运行排队或同日补跑 | 使用同一日期和 SHA。已发布时复用资产，清单更新失败时可补写，较晚日期发布后重跑旧日仍保留较新的清单。 |
| CI、自检、签名、上传或检出失败 | 上一份有效清单继续可用。发布成功的 Nightly 仍为 Pre-release，`/releases/latest/download/latest.json` 仍指向正式版。 |
| 正式版客户端默认关闭、开启、再关闭订阅 | 默认只看正式版。开启后能收到更高 Nightly，关闭立刻撤下尚未安装的 Nightly 提示。重启后沿用已保存选择。 |
| 关闭自动检查但开启 Nightly | 定时检查停用。「立即检查」仍可找到 Nightly。 |
| Nightly 清单故障，正式清单可用 | 正式更新可照常提示。有失败但无候选时，手动检查报告故障渠道。 |
| 保存订阅失败，或界面尝试安装已失效的候选 | 保存失败时按上次保存成功的设置检查更新。安装请求只能消费当前待安装项。 |
| Windows 自定义安装目录中执行正式版 → Nightly → 次日 Nightly → 对应正式版 | 版本顺序正确，软件更新设置、关于页和实际运行包版本一致，安装目录及用户数据得以保留。签名损坏的更新无法安装。 |

方案阶段已验证北京时间窗口边界、UTC 定时时刻与上述 SemVer 顺序，并对照了项目锁定的 Tauri Updater 实现。表中的发布与 Windows 安装场景用于实施后验收。

本地验证覆盖按日选取提交、无变更跳过，以及补跑已发布版本时恢复清单、保留较新清单。四语校验通过。Chromium 中使用本地 IPC 测试桩验证了开启订阅后提示更新与取消后撤下提示。GitHub 实际发布和 Windows 签名安装包的原位升级仍需上线验收。
