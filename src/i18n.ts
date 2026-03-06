import { ref, watch } from "vue";

export type Locale = "zh" | "en";

interface MessageTree {
  [key: string]: string | MessageTree;
}

const STORAGE_KEY = "disk-relocator.locale";

const messages: Record<Locale, MessageTree> = {
  zh: {
    common: {
      zh: "中文",
      en: "English",
      language: "语言",
      cancelled: "取消",
      confirm: "确认",
      refresh: "刷新",
      refreshing: "刷新中..."
    },
    app: {
      subtitle: "macOS 空间释放工具",
      sidebar: {
        apps: "应用列表",
        health: "健康检查",
        logs: "操作记录"
      },
      sourcePanel: {
        title: "数据源",
        appScan: "应用扫描：{count} 项",
        diskDetected: "已检测磁盘：{count} 项",
        migratableTargets: "可迁移目标：{count} 项"
      },
      systemDisk: {
        title: "内置磁盘",
        free: "剩余 {free}",
        total: "总计 {total}",
        unavailable: "暂无法读取系统盘容量"
      },
      messages: {
        loadFailed: "数据加载失败：{error}",
        restoreConfirm: "确定要将 {name} 的数据恢复到系统盘吗？",
        rollbackRecordMissing: "未找到 {name} 的回滚记录。",
        restoreDone: "{name} 已回滚到系统盘。",
        restoreFailed: "回滚失败：{error}",
        migrationDone: "迁移完成：{label}"
      },
      pathFallback: "(未检测到路径)",
      descFallback: "应用数据目录迁移项",
      appDescriptions: {
        "wechat-non-mas": "仅迁移微信主容器（聊天记录与媒体数据主体）。",
        "telegram-desktop": "包含 Telegram 聊天缓存与媒体文件。",
        "jetbrains-caches": "JetBrains IDE 缓存目录，迁移后可释放大量系统盘空间。",
        "xcode-derived-data": "Xcode DerivedData 构建缓存目录。",
        "mas-sandbox-containers": "系统策略限制，不支持软链接迁移。",
        "docker-desktop-data-root": "当前策略不支持软链接迁移，请使用 Docker 原生 data-root。"
      }
    },
    appList: {
      title: "应用搬迁",
      subtitle: "真实扫描应用数据目录，支持迁移到外接盘并可回滚。",
      refresh: "刷新扫描",
      refreshing: "刷新中...",
      migrated: "已外存",
      migratedTo: "已外存至 {disk}",
      tier: {
        experimental: "实验支持",
        blocked: "已禁用"
      },
      hint: {
        blocked: "当前画像为 blocked，不支持迁移",
        running: "应用正在运行，请先完全退出再迁移",
        experimental: "实验支持，迁移前需确认风险",
        migrated: "已检测到软链接迁移状态",
        ready: "可直接迁移"
      },
      sizeLabel: "目录大小",
      migrate: "搬迁外存",
      restore: "恢复到系统",
      empty: "未检测到可识别应用。请先启动一次目标应用后再刷新扫描。"
    },
    migrationDialog: {
      title: "迁移 {name}",
      subtitle: "系统将自动移动数据目录并建立软链接，此过程安全可逆。",
      sizeLabel: "需要迁移的数据量",
      diskLabel: "选择目标外接磁盘",
      diskHint: "仅显示“已挂载且可写”的磁盘。",
      diskFree: "可用 {size}",
      diskNone: "当前没有可用于迁移的目标磁盘（可能未挂载或不可写）。",
      targetRootLabel: "目标盘根路径",
      targetRootDiskRoot: "{root}（盘根目录）",
      targetRootRecommended: "{root}/DataDock（推荐）",
      targetRootRelocator: "{root}/RelocatorData",
      targetRootSystemPick: "{root}（系统选择）",
      pickTitle: "选择目标盘根路径",
      pickButton: "系统选择",
      picking: "选择中...",
      selectDiskFirst: "请先选择目标盘，再使用系统选择。",
      pathNotInDisk: "所选路径不在目标盘 {disk} 下，请在该盘内选择。",
      allowExperimental: "我已知晓 experimental 风险并允许执行迁移。",
      cleanupBackup: "迁移成功后清理本地备份（.bak），释放系统盘空间。",
      warning: "迁移过程中请勿拔除外接硬盘，也请确保 {name} 已经完全退出。",
      skippedTitle: "本次将跳过：",
      cancel: "取消",
      start: "开始迁移",
      migratingBtn: "迁移中...",
      migratingTitle: "正在迁移数据...",
      migratingSubtitle: "正在将 {count} 个数据目录迁移至 {disk}",
      keepDiskOnline: "请保持目标磁盘在线",
      successTitle: "迁移成功！",
      successText: "{name} 的数据已安全转移至 {disk}。本次成功执行 {count} 个迁移任务。",
      finish: "完成",
      reason: {
        blocked: "blocked，不允许迁移",
        running: "应用运行中，请先退出",
        migrated: "源目录已是软链接，已迁移",
        sourceMissingNoData: "未检测到源目录，当前应用还没有可迁移的数据",
        sourceMissingBootstrap: "未检测到源目录，将自动完成初始化后继续迁移",
        sourceEmptyBootstrap: "源目录暂无数据，将自动完成初始化后继续迁移",
        sourceDetected: "检测到源数据，准备迁移"
      },
      errors: {
        startFailed: "迁移失败：{error}"
      },
      doneMessage: "迁移完成：{label}",
      fallbackLabel: "目标应用"
    },
    health: {
      title: "健康检查与诊断",
      subtitle: "实时监控外接磁盘状态与软链接完整性（10 秒自动刷新）。",
      checkNow: "健康自检（自动纠偏）",
      checking: "自检中...",
      allHealthy: "所有软链接正常",
      issueCount: "检测到 {count} 项异常",
      summary: "已检查 {total} 项，其中健康 {healthy} 项。",
      disksOnlineCount: "{count} 个可迁移目标盘",
      noDisk: "暂无可迁移目标盘",
      diskFilterHint: "仅显示“已挂载且可写”的目标盘（与搬迁弹窗一致）。",
      diskCapacity: "可用 {free} / 总计 {total}",
      mountPointHint: "请连接并挂载可写目标磁盘后重试。",
      tableTitle: "异常项与回滚",
      table: {
        app: "应用 / relocation",
        link: "链接状态",
        disk: "挂载盘状态",
        action: "操作"
      },
      state: {
        healthy: "健康",
        degraded: "降级",
        broken: "故障",
        mounted: "已挂载",
        unmounted: "未挂载",
        readonly: "只读"
      },
      recheck: "运行自检",
      rollback: "一键回滚",
      rollbacking: "回滚中...",
      empty: "暂无健康检查数据。",
      recentEvents: "最近健康事件",
      infoRollbackDone: "已执行回滚：{id}",
      infoSelfCheckDone:
        "自检完成：发现 {drift} 项漂移，已自动纠偏 {fixed} 项，剩余 {remaining} 项需手动处理。",
      errorRefreshFailed: "健康检查失败：{error}",
      errorSelfCheckFailed: "健康自检失败：{error}",
      errorRollbackFailed: "回滚失败：{error}"
    },
    logs: {
      title: "操作记录",
      subtitle: "查看迁移/回滚结果与失败原因。",
      refreshRecords: "刷新记录",
      refreshingRecords: "刷新中...",
      recordsTitle: "迁移与回滚记录",
      recordsSubtitle: "展示用户可读结果；失败时可展开查看关键步骤日志。",
      appFilter: "应用",
      appFilterAll: "全部应用",
      operationType: "操作类型",
      operationTypeAll: "全部",
      operationTypeMigrate: "迁移",
      operationTypeRollback: "回滚",
      unknownApp: "未知应用",
      action: {
        migrate: "迁移",
        rollback: "回滚",
        unknown: "未知操作"
      },
      result: {
        success: "成功",
        failed: "失败",
        running: "进行中"
      },
      timeRange: "时间",
      lastStep: "最后步骤",
      failureTitle: "失败详情",
      failureCode: "错误码",
      failureShow: "查看失败日志",
      failureHide: "收起失败日志",
      recordsEmpty: "暂无操作记录。",
      errorListFailed: "拉取操作记录失败：{error}"
    }
  },
  en: {
    common: {
      zh: "Chinese",
      en: "English",
      language: "Language",
      cancelled: "Cancel",
      confirm: "Confirm",
      refresh: "Refresh",
      refreshing: "Refreshing..."
    },
    app: {
      subtitle: "macOS Storage Relief Tool",
      sidebar: {
        apps: "Applications",
        health: "Health Check",
        logs: "Operation History"
      },
      sourcePanel: {
        title: "Data Sources",
        appScan: "Scanned apps: {count}",
        diskDetected: "Detected disks: {count}",
        migratableTargets: "Migratable targets: {count}"
      },
      systemDisk: {
        title: "Built-in Disk",
        free: "Free {free}",
        total: "Total {total}",
        unavailable: "System disk capacity is currently unavailable"
      },
      messages: {
        loadFailed: "Failed to load data: {error}",
        restoreConfirm: "Restore data of {name} back to system disk?",
        rollbackRecordMissing: "No rollback record found for {name}.",
        restoreDone: "{name} has been restored to the system disk.",
        restoreFailed: "Rollback failed: {error}",
        migrationDone: "Migration completed: {label}"
      },
      pathFallback: "(path not detected)",
      descFallback: "App data relocation profile",
      appDescriptions: {
        "wechat-non-mas":
          "Move WeChat main container only (primary chat and media data).",
        "telegram-desktop": "Includes Telegram chat cache and media files.",
        "jetbrains-caches":
          "JetBrains IDE cache directory. Migration can free significant system disk space.",
        "xcode-derived-data": "Xcode DerivedData build cache directory.",
        "mas-sandbox-containers":
          "Restricted by system policy. Symlink migration is not supported.",
        "docker-desktop-data-root":
          "Symlink migration is not supported. Use Docker native data-root instead."
      }
    },
    appList: {
      title: "App Migration",
      subtitle:
        "Scan real app data directories and relocate them to external disks with rollback support.",
      refresh: "Refresh Scan",
      refreshing: "Refreshing...",
      migrated: "Relocated",
      migratedTo: "Relocated to {disk}",
      tier: {
        experimental: "Experimental",
        blocked: "Blocked"
      },
      hint: {
        blocked: "Current profile is blocked and cannot be migrated",
        running: "App is running. Please quit it before migration",
        experimental: "Experimental support. Confirm risk before migration",
        migrated: "Symlink migration state already detected",
        ready: "Ready to migrate"
      },
      sizeLabel: "Directory Size",
      migrate: "Move to External",
      restore: "Restore to System",
      empty: "No recognized app found. Launch the target app once and refresh."
    },
    migrationDialog: {
      title: "Migrate {name}",
      subtitle:
        "The system will move the data directory and create a symlink automatically. This is safe and reversible.",
      sizeLabel: "Data Size to Migrate",
      diskLabel: "Select Target External Disk",
      diskHint: "Only mounted and writable disks are listed.",
      diskFree: "Free {size}",
      diskNone: "No available target disk for migration (possibly unmounted or read-only).",
      targetRootLabel: "Target Root Path",
      targetRootDiskRoot: "{root} (disk root)",
      targetRootRecommended: "{root}/DataDock (recommended)",
      targetRootRelocator: "{root}/RelocatorData",
      targetRootSystemPick: "{root} (picked from system)",
      pickTitle: "Select target root path",
      pickButton: "System Picker",
      picking: "Picking...",
      selectDiskFirst: "Select a target disk before using system picker.",
      pathNotInDisk: "Selected path is not under disk {disk}. Please pick inside that disk.",
      allowExperimental: "I understand experimental risk and allow migration.",
      cleanupBackup: "Cleanup local backup (.bak) after success to free system disk space.",
      warning:
        "Do not unplug the external disk during migration. Also make sure {name} is fully quit.",
      skippedTitle: "Will skip:",
      cancel: "Cancel",
      start: "Start Migration",
      migratingBtn: "Migrating...",
      migratingTitle: "Migrating data...",
      migratingSubtitle: "Migrating {count} data directories to {disk}",
      keepDiskOnline: "Keep target disk online",
      successTitle: "Migration Succeeded!",
      successText:
        "{name} data has been moved to {disk}. {count} migration task(s) succeeded.",
      finish: "Done",
      reason: {
        blocked: "blocked, migration is not allowed",
        running: "app is running, please quit first",
        migrated: "source path is already a symlink",
        sourceMissingNoData: "source path not detected, no migratable data yet",
        sourceMissingBootstrap:
          "source path not detected, initialization will be handled automatically",
        sourceEmptyBootstrap:
          "source path has no data, initialization will be handled automatically",
        sourceDetected: "source data detected, ready to migrate"
      },
      errors: {
        startFailed: "Migration failed: {error}"
      },
      doneMessage: "Migration completed: {label}",
      fallbackLabel: "target app"
    },
    health: {
      title: "Health Check & Diagnostics",
      subtitle:
        "Monitor external disk status and symlink integrity in real time (auto refresh every 10s).",
      checkNow: "Health Self-check (Auto-heal)",
      checking: "Self-checking...",
      allHealthy: "All symlinks healthy",
      issueCount: "{count} issue(s) detected",
      summary: "{total} item(s) checked, {healthy} healthy.",
      disksOnlineCount: "{count} migratable target disk(s)",
      noDisk: "No migratable target disk",
      diskFilterHint: "Only mounted and writable target disks are shown (same as migration dialog).",
      diskCapacity: "Free {free} / Total {total}",
      mountPointHint: "Connect and mount a writable target disk, then retry.",
      tableTitle: "Issues & Rollback",
      table: {
        app: "App / relocation",
        link: "Link Status",
        disk: "Mounted Disk",
        action: "Action"
      },
      state: {
        healthy: "Healthy",
        degraded: "Degraded",
        broken: "Broken",
        mounted: "Mounted",
        unmounted: "Unmounted",
        readonly: "Read-only"
      },
      recheck: "Run Self-check",
      rollback: "Rollback",
      rollbacking: "Rolling back...",
      empty: "No health data.",
      recentEvents: "Recent Health Events",
      infoRollbackDone: "Rollback executed: {id}",
      infoSelfCheckDone:
        "Self-check done: {drift} drift issue(s), {fixed} auto-healed, {remaining} remaining for manual handling.",
      errorRefreshFailed: "Health check failed: {error}",
      errorSelfCheckFailed: "Health self-check failed: {error}",
      errorRollbackFailed: "Rollback failed: {error}"
    },
    logs: {
      title: "Operation History",
      subtitle: "View migration/rollback outcomes and failure reasons.",
      refreshRecords: "Refresh Records",
      refreshingRecords: "Refreshing...",
      recordsTitle: "Migration & Rollback History",
      recordsSubtitle: "Shows user-readable outcomes; failed records can expand key step logs.",
      appFilter: "Application",
      appFilterAll: "All Apps",
      operationType: "Operation Type",
      operationTypeAll: "All",
      operationTypeMigrate: "Migration",
      operationTypeRollback: "Rollback",
      unknownApp: "Unknown App",
      action: {
        migrate: "Migration",
        rollback: "Rollback",
        unknown: "Unknown"
      },
      result: {
        success: "Success",
        failed: "Failed",
        running: "Running"
      },
      timeRange: "Time",
      lastStep: "Last Step",
      failureTitle: "Failure Details",
      failureCode: "Error Code",
      failureShow: "Show Failure Logs",
      failureHide: "Hide Failure Logs",
      recordsEmpty: "No operation records.",
      errorListFailed: "Failed to load operation records: {error}"
    }
  }
};

function readInitialLocale(): Locale {
  if (typeof window === "undefined") {
    return "zh";
  }
  const stored = window.localStorage.getItem(STORAGE_KEY);
  if (stored === "zh" || stored === "en") {
    return stored;
  }
  return "zh";
}

const locale = ref<Locale>(readInitialLocale());

watch(locale, (next) => {
  if (typeof window !== "undefined") {
    window.localStorage.setItem(STORAGE_KEY, next);
  }
});

function resolveMessage(path: string, targetLocale: Locale): string | undefined {
  const parts = path.split(".");
  let cursor: string | MessageTree | undefined = messages[targetLocale];
  for (const part of parts) {
    if (!cursor || typeof cursor === "string") {
      return undefined;
    }
    cursor = cursor[part];
  }
  return typeof cursor === "string" ? cursor : undefined;
}

function format(template: string, params?: Record<string, string | number>): string {
  if (!params) {
    return template;
  }
  return template.replace(/\{([a-zA-Z0-9_]+)\}/g, (_, token: string) => {
    if (Object.prototype.hasOwnProperty.call(params, token)) {
      return String(params[token]);
    }
    return `{${token}}`;
  });
}

export function useI18n() {
  function t(path: string, params?: Record<string, string | number>): string {
    const localized =
      resolveMessage(path, locale.value) ??
      resolveMessage(path, locale.value === "zh" ? "en" : "zh") ??
      path;
    return format(localized, params);
  }

  function setLocale(next: Locale): void {
    locale.value = next;
  }

  return {
    locale,
    setLocale,
    t
  };
}
