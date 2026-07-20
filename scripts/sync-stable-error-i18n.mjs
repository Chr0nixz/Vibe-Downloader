import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

// Load TS module via vitest/tsx isn't available; duplicate the minimal maps here
// by importing the compiled source through dynamic import of the .ts via jiti-less parse.
// Instead, read stable-error-codes.ts as text and eval the EN messages object via a tiny transform.

const stablePath = path.resolve("src/lib/stable-error-codes.ts");
const stableSrc = fs.readFileSync(stablePath, "utf8");

function extractArray(name) {
  const match = stableSrc.match(new RegExp(`export const ${name} = \\[([\\s\\S]*?)\\] as const;`));
  if (!match) throw new Error(`Missing ${name}`);
  return [...match[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

function extractEnMessages() {
  const match = stableSrc.match(
    /export const STABLE_ERROR_MESSAGES_EN[^=]*= \{([\s\S]*?)\n\};/,
  );
  if (!match) throw new Error("Missing STABLE_ERROR_MESSAGES_EN");
  const obj = {};
  for (const m of match[1].matchAll(/^\s*([a-z0-9_]+):\s*"((?:\\.|[^"\\])*)"/gm)) {
    obj[m[1]] = m[2].replace(/\\"/g, '"').replace(/\\n/g, "\n");
  }
  return obj;
}

const STABLE_ERROR_CODES = extractArray("STABLE_ERROR_CODES");
const STABLE_ERROR_MESSAGES_EN = extractEnMessages();

function camelFromCode(code) {
  return code.replace(/_([a-z])/g, (_, ch) => ch.toUpperCase());
}

const zhCN = {
  ...STABLE_ERROR_MESSAGES_EN,
  auth_headers_expired: "认证凭据已过期。请检查 URL 后重试。",
  auth_headers_unavailable: "无法使用认证凭据。请提供凭据后重试。",
  body_error: "无法读取响应内容。请重试下载。",
  bt_file_selection_required: "开始此种子前请至少选择一个文件。",
  bt_metadata_failed: "无法获取 BitTorrent 元数据。",
  bt_metadata_timeout: "等待 BitTorrent 元数据超时。",
  bt_runtime_stats_failed: "无法获取 BitTorrent 运行时统计。",
  bt_source_failed: "无法加载种子或磁力链接源。",
  canceled: "下载已取消。",
  connection_refused: "远程主机拒绝了连接。",
  dash_empty_output: "DASH 下载产出了空文件。",
  dash_ffmpeg_failed: "ffmpeg 处理 DASH 流失败。",
  dash_ffmpeg_missing: "DASH 下载需要 ffmpeg。请在设置中配置。",
  dash_invalid_manifest: "DASH 清单无效或不支持。",
  dash_live_unsupported: "暂不支持直播 DASH 流。",
  dash_no_duration: "DASH 清单未声明时长。",
  dash_no_segments: "DASH 清单中找不到可下载分段。",
  dash_no_tracks: "DASH 清单中找不到音视频轨道。",
  dash_segment_failed: "DASH 分段下载失败。",
  dash_segment_stalled: "DASH 分段下载停滞。",
  dash_segment_timeline_unsupported: "暂不支持此 DASH SegmentTimeline。",
  decode_error: "无法解码响应内容。",
  disk_write_failed: "写入磁盘失败。请释放空间或更换文件夹。",
  dns_failure: "DNS 解析失败。请检查主机名和网络。",
  download_failed: "下载失败。",
  duplicate_task: "该资源已存在下载任务。",
  ffmpeg_missing: "此任务需要 ffmpeg。请在设置中配置。",
  ffmpeg_not_found: "未找到 ffmpeg。请在设置中配置路径。",
  final_path_conflict: "目标文件已存在。请更换文件名或文件夹。",
  ftp_auth_failed: "FTP 认证失败。请检查用户名和密码。",
  ftp_connect_failed: "无法连接 FTP 服务器。",
  ftp_proxy_unsupported_for_implicit_tls: "当前代理不支持隐式 FTPS。",
  ftp_read_failed: "从 FTP 服务器读取失败。",
  ftp_read_timeout: "FTP 读取超时。",
  ftp_tls_handshake_failed: "FTP TLS 握手失败。",
  hls_ffmpeg_missing: "HLS 封装需要 ffmpeg。请在设置中配置。",
  hls_init_too_large: "HLS 初始化分段过大。",
  hls_invalid_playlist: "HLS 播放列表无效或不支持。",
  hls_segment_failed: "HLS 分段下载失败。",
  hls_segment_stalled: "HLS 分段下载停滞。",
  hls_segment_too_large: "HLS 分段超过大小限制。",
  hls_unsupported_encryption: "不支持此 HLS 加密方式。",
  http_denied: "访问被拒绝（403）。请检查 URL 或凭据。",
  http_not_found: "资源未找到（404）。请检查 URL。",
  io_error: "发生本地文件 I/O 错误。",
  metalink_all_mirrors_failed: "所有 Metalink 镜像均失败。",
  metalink_invalid_manifest: "Metalink 清单无效。",
  metalink_mirror_unsupported_range: "某个 Metalink 镜像不支持断点续传。",
  metalink_no_files: "Metalink 清单中没有文件。",
  metalink_no_healthy_mirrors: "没有可用的 Metalink 镜像。",
  metalink_no_resources: "该 Metalink 文件没有可下载资源。",
  metalink_partial_completion: "Metalink 下载仅部分完成。",
  network_error: "网络错误中断了传输。",
  network_unreachable: "无法到达远程网络。",
  proxy_auth_failed: "代理认证失败。请检查代理凭据。",
  proxy_configuration_invalid: "代理配置无效。",
  proxy_connect_failed: "无法通过代理连接。",
  proxy_connection_failed: "代理连接失败。",
  proxy_scheme_unsupported_for_task: "此代理协议不支持该任务协议。",
  proxy_secret_decrypt_failed: "无法解密已保存的代理凭据。",
  proxy_secret_encrypt_failed: "无法加密代理凭据以供存储。",
  proxy_timeout: "代理连接超时。",
  redirect_error: "跟随重定向失败。",
  remote_changed: "远程文件自上次下载后已变化。请从头重新开始。",
  resume_mismatch: "续传校验失败，远程文件已不匹配。",
  resume_unavailable: "服务器不再支持续传。请从头重新开始。",
  server_rate_limited: "服务器正在限制请求频率。请稍后再试。",
  sftp_auth_failed: "SFTP 认证失败。请检查凭据或密钥。",
  sftp_channel_failed: "打开 SFTP 通道失败。",
  sftp_connect_failed: "无法连接 SFTP 服务器。",
  sftp_credentials_required: "需要 SFTP 凭据。",
  sftp_directory_not_file: "SFTP 路径是目录而不是文件。",
  sftp_directory_probe_failed: "探测 SFTP 目录失败。",
  sftp_host_key_changed: "SFTP 主机密钥已变更。仅在信任新密钥时清除旧记录。",
  sftp_invalid_url: "SFTP URL 无效。",
  sftp_open_failed: "打开远程 SFTP 文件失败。",
  sftp_probe_state_unavailable: "SFTP 探测状态不可用。",
  sftp_proxy_unsupported: "当前代理不支持 SFTP。",
  sftp_read_failed: "从 SFTP 服务器读取失败。",
  sftp_read_timeout: "SFTP 读取超时。",
  sftp_resume_failed: "恢复 SFTP 下载失败。",
  sftp_short_read: "SFTP 服务器返回的字节数少于预期。",
  sftp_size_mismatch: "远程 SFTP 文件大小已不匹配。",
  sftp_stat_failed: "获取远程 SFTP 文件信息失败。",
  sftp_subsystem_failed: "启动 SFTP 子系统失败。",
  task_credentials_decrypt_failed: "无法解密已保存的任务凭据。",
  task_credentials_encrypt_failed: "无法加密任务凭据以供存储。",
  task_credentials_invalid: "任务凭据无效。",
  task_credentials_unavailable: "任务凭据不可用。",
  temp_file_missing: "临时下载文件缺失。请重新开始下载。",
  temp_file_smaller_than_progress: "临时文件小于已记录进度。请重新开始下载。",
  timeout: "请求超时。",
  tls_error: "连接时发生 TLS/SSL 错误。",
  unknown_error: "发生意外错误。",
  webdav_invalid_multistatus: "WebDAV 多状态响应无效。",
  webdav_propfind_failed: "WebDAV PROPFIND 失败。",
};

const reportLocales = {
  en: {
    taskId: "Task ID",
    url: "URL",
    code: "Code",
    message: "Message",
    recoverable: "Recoverable",
    actions: "Actions",
  },
  "zh-CN": {
    taskId: "任务 ID",
    url: "URL",
    code: "错误码",
    message: "消息",
    recoverable: "可恢复",
    actions: "动作",
  },
  "zh-TW": {
    taskId: "任務 ID",
    url: "URL",
    code: "錯誤碼",
    message: "訊息",
    recoverable: "可恢復",
    actions: "動作",
  },
  ja: {
    taskId: "タスク ID",
    url: "URL",
    code: "コード",
    message: "メッセージ",
    recoverable: "回復可能",
    actions: "アクション",
  },
  ko: {
    taskId: "작업 ID",
    url: "URL",
    code: "코드",
    message: "메시지",
    recoverable: "복구 가능",
    actions: "동작",
  },
  ru: {
    taskId: "ID задачи",
    url: "URL",
    code: "Код",
    message: "Сообщение",
    recoverable: "Восстановимо",
    actions: "Действия",
  },
  es: {
    taskId: "ID de tarea",
    url: "URL",
    code: "Código",
    message: "Mensaje",
    recoverable: "Recuperable",
    actions: "Acciones",
  },
};

function buildBlock(locale, messages) {
  const report = reportLocales[locale];
  const lines = ["  errors: {"];
  for (const code of STABLE_ERROR_CODES) {
    const key = camelFromCode(code);
    const raw = messages[code] ?? STABLE_ERROR_MESSAGES_EN[code];
    lines.push(`    ${key}: ${JSON.stringify(raw)},`);
  }
  lines.push("    report: {");
  for (const [k, v] of Object.entries(report)) {
    lines.push(`      ${k}: ${JSON.stringify(v)},`);
  }
  lines.push("    },");
  lines.push("  },");
  return lines.join("\n");
}

const messageSets = {
  en: STABLE_ERROR_MESSAGES_EN,
  "zh-CN": zhCN,
  "zh-TW": STABLE_ERROR_MESSAGES_EN,
  ja: STABLE_ERROR_MESSAGES_EN,
  ko: STABLE_ERROR_MESSAGES_EN,
  ru: STABLE_ERROR_MESSAGES_EN,
  es: STABLE_ERROR_MESSAGES_EN,
};

const root = path.resolve("src/i18n/locales");
let failed = 0;
for (const locale of Object.keys(messageSets)) {
  const file = path.join(root, `${locale}.ts`);
  let text = fs.readFileSync(file, "utf8");
  // Normalize CRLF so the errors-block regex matches on Windows checkouts.
  text = text.replace(/\r\n/g, "\n");
  const block = buildBlock(locale, messageSets[locale]);
  const replaced = text.replace(/  errors: \{[\s\S]*?\n  \},\n\} as const;/, `${block}\n} as const;`);
  if (replaced === text) {
    // Already synced (identical block) or regex miss — distinguish by presence of a new key.
    if (text.includes("tempFileSmallerThanProgress:")) {
      console.log("Unchanged", file, "(already synced)");
      continue;
    }
    console.error("Failed to replace errors block in", file);
    failed += 1;
    continue;
  }
  fs.writeFileSync(file, replaced.replace(/\n/g, "\r\n"));
  console.log("Updated", file, "codes=", STABLE_ERROR_CODES.length);
}
if (failed > 0) process.exit(1);
