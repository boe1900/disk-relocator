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
      version: "版本",
      cancelled: "取消",
      confirm: "确认",
      close: "关闭",
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
        restoreBlockedRunning: "{name} 正在运行，请先完全退出后再恢复到系统盘。",
        rollbackRecordMissing: "未找到 {name} 的回滚记录。",
        restoreDone: "{name} 已回滚到系统盘。",
        restoreFailed: "回滚失败：{error}",
        migrationDone: "迁移完成：{label}",
        unknownReason: "未知原因"
      },
      pathFallback: "(未检测到路径)",
      descFallback: "应用数据目录迁移项"
    },
    appList: {
      title: "应用搬迁",
      subtitle: "真实扫描应用数据目录，支持迁移到外接盘并可回滚。",
      refresh: "刷新扫描",
      refreshing: "刷新中...",
      migrated: "已外存",
      migratedTo: "已外存至 {disk}",
      status: {
        requiresConfirmation: "需确认",
        blocked: "已禁用",
        deprecated: "已弃用"
      },
      hint: {
        blocked: "当前画像为 blocked，不支持迁移",
        blockedWithReason: "当前画像被阻断：{reason}",
        deprecated: "当前画像已弃用，默认不支持新迁移",
        noExecutableUnit: "当前没有可迁移的数据单元",
        running: "应用正在运行，请先完全退出后再执行迁移/恢复",
        requiresConfirmation: "包含需确认的数据单元，迁移前请确认风险",
        migrated: "已检测到软链接迁移状态",
        ready: "可直接迁移"
      },
      sizeLabel: "预计释放空间",
      sizeLabelCurrent: "预计释放空间",
      sizeLabelSaved: "已释放空间",
      pathCount: "{count} 个目录",
      pathGroup: {
        default: "默认目录",
        account: "账号 {account}"
      },
      migrate: "搬迁外存",
      restore: "恢复到系统",
      pathActions: {
        openInFinder: "在 Finder 打开",
        copyPath: "复制路径",
        viewDetails: "查看目录详情",
        pendingBadge: "待迁移 {count}",
        copied: "已复制",
        openFailed: "在 Finder 打开失败：{error}",
        copyFailed: "复制路径失败：{error}"
      },
      pathStatus: {
        migrated: "已迁移",
        pending: "未迁移"
      },
      pathDetails: {
        title: "{name} 目录详情",
        subtitle: "共 {count} 个目录",
        close: "关闭"
      },
      empty: "未检测到可识别应用。请先启动一次目标应用后再刷新扫描。"
    },
    migrationDialog: {
      title: "迁移 {name}",
      subtitle: "系统将自动移动数据目录并建立软链接，此过程安全可逆。",
      sizeLabel: "需要迁移的数据量",
      unitLabel: "迁移目录单元",
      unitHint: "已选 {selected}/{total}",
      selectAllUnits: "全选",
      clearUnits: "清空",
      openInFinder: "在 Finder 打开",
      copyPath: "复制路径",
      pathCopied: "已复制",
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
      confirmHighRisk: "我已知晓高风险单元并允许执行迁移。",
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
      pathGroup: {
        default: "默认分组",
        account: "账号 {account}"
      },
      errors: {
        startFailed: "迁移失败：{error}",
        copyPathFailed: "复制路径失败：{error}",
        openPathFailed: "在 Finder 打开失败：{error}",
        targetPathExists:
          "目标目录已存在：{path}。为避免覆盖，已停止迁移。可更换目标目录，或先在 Finder 中重命名/移动该目录后重试。",
        targetPathExistsNoPath:
          "目标目录已存在。为避免覆盖，已停止迁移。可更换目标目录，或先处理旧目录后重试。",
        backupPathExists:
          "备份目录已存在：{path}。为避免覆盖，已停止迁移。请先处理该目录后再试。",
        backupPathExistsNoPath: "备份目录已存在。为避免覆盖，已停止迁移。请先处理该目录后再试。"
      },
      riskWarning: {
        title: "高风险操作警告",
        defaultMessage:
          "当前迁移包含高风险目录。迁移到外接盘后，请勿在应用运行时拔盘；拔盘前请先彻底退出应用（Command + Q），否则可能导致数据损坏。",
        confirm: "我已知晓风险，继续",
        confirmWithCountdown: "我已知晓风险（{seconds}s）"
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
      tableTitle: "异常项",
      table: {
        app: "应用 / relocation",
        link: "链接状态",
        disk: "挂载盘状态",
        action: "操作"
      },
      groupCount: "{count} 项迁移记录",
      userLabel: "账号 {account}",
      pathActions: {
        openInFinder: "在 Finder 打开",
        copyPath: "复制路径",
        copied: "已复制",
        pathUnavailable: "迁移路径信息不可用。",
        openFailed: "在 Finder 打开失败：{error}",
        copyFailed: "复制路径失败：{error}"
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
        "自检完成：当前严重异常 {drift} 项，已自动处理 {fixed} 项，剩余 {remaining} 项需手动处理。",
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
      unitId: "数据单元",
      migrateSourcePath: "迁移目录",
      rollbackSourcePath: "回滚目录",
      targetPath: "外存目录",
      pathUnavailable: "目录信息不可用",
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
      version: "Version",
      cancelled: "Cancel",
      confirm: "Confirm",
      close: "Close",
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
        restoreBlockedRunning: "{name} is running. Quit it before restoring to system disk.",
        rollbackRecordMissing: "No rollback record found for {name}.",
        restoreDone: "{name} has been restored to the system disk.",
        restoreFailed: "Rollback failed: {error}",
        migrationDone: "Migration completed: {label}",
        unknownReason: "unknown reason"
      },
      pathFallback: "(path not detected)",
      descFallback: "App data relocation profile"
    },
    appList: {
      title: "App Migration",
      subtitle:
        "Scan real app data directories and relocate them to external disks with rollback support.",
      refresh: "Refresh Scan",
      refreshing: "Refreshing...",
      migrated: "Relocated",
      migratedTo: "Relocated to {disk}",
      status: {
        requiresConfirmation: "Needs Confirmation",
        blocked: "Blocked",
        deprecated: "Deprecated"
      },
      hint: {
        blocked: "Current profile is blocked and cannot be migrated",
        blockedWithReason: "Current profile is blocked: {reason}",
        deprecated: "Current profile is deprecated and does not allow new migration",
        noExecutableUnit: "No executable relocation unit is currently available",
        running: "App is running. Quit it before migrate/restore operations",
        requiresConfirmation: "Contains units that require confirmation before migration",
        migrated: "Symlink migration state already detected",
        ready: "Ready to migrate"
      },
      sizeLabel: "Estimated Releasable Space",
      sizeLabelCurrent: "Estimated Releasable Space",
      sizeLabelSaved: "Released Space",
      pathCount: "{count} directories",
      pathGroup: {
        default: "Default",
        account: "Account {account}"
      },
      migrate: "Move to External",
      restore: "Restore to System",
      pathActions: {
        openInFinder: "Show in Finder",
        copyPath: "Copy Path",
        viewDetails: "View Details",
        pendingBadge: "Pending {count}",
        copied: "Copied",
        openFailed: "Failed to show path in Finder: {error}",
        copyFailed: "Failed to copy path: {error}"
      },
      pathStatus: {
        migrated: "Migrated",
        pending: "Pending"
      },
      pathDetails: {
        title: "{name} Path Details",
        subtitle: "{count} paths",
        close: "Close"
      },
      empty: "No recognized app found. Launch the target app once and refresh."
    },
    migrationDialog: {
      title: "Migrate {name}",
      subtitle:
        "The system will move the data directory and create a symlink automatically. This is safe and reversible.",
      sizeLabel: "Data Size to Migrate",
      unitLabel: "Relocation Units",
      unitHint: "{selected}/{total} selected",
      selectAllUnits: "Select All",
      clearUnits: "Clear",
      openInFinder: "Show in Finder",
      copyPath: "Copy Path",
      pathCopied: "Copied",
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
      confirmHighRisk: "I understand the high-risk unit and allow migration.",
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
      pathGroup: {
        default: "Default",
        account: "Account {account}"
      },
      errors: {
        startFailed: "Migration failed: {error}",
        copyPathFailed: "Failed to copy path: {error}",
        openPathFailed: "Failed to show path in Finder: {error}",
        targetPathExists:
          "Target directory already exists: {path}. Migration is stopped to avoid overwriting. Choose another target path, or rename/move this directory in Finder and retry.",
        targetPathExistsNoPath:
          "Target directory already exists. Migration is stopped to avoid overwriting. Choose another target path, or handle the existing directory and retry.",
        backupPathExists:
          "Backup directory already exists: {path}. Migration is stopped to avoid overwriting. Handle this directory and retry.",
        backupPathExistsNoPath:
          "Backup directory already exists. Migration is stopped to avoid overwriting. Handle this directory and retry."
      },
      riskWarning: {
        title: "High-Risk Migration Warning",
        defaultMessage:
          "This migration includes high-risk data directories. After moving to an external disk, never unplug while the app is running. Fully quit the app (Command + Q) before unplugging, or data may be damaged.",
        confirm: "I understand, continue",
        confirmWithCountdown: "I understand ({seconds}s)"
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
      tableTitle: "Issues",
      table: {
        app: "App / relocation",
        link: "Link Status",
        disk: "Mounted Disk",
        action: "Action"
      },
      groupCount: "{count} relocation item(s)",
      userLabel: "Account {account}",
      pathActions: {
        openInFinder: "Show in Finder",
        copyPath: "Copy Path",
        copied: "Copied",
        pathUnavailable: "Migration path info unavailable.",
        openFailed: "Failed to show path in Finder: {error}",
        copyFailed: "Failed to copy path: {error}"
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
        "Self-check done: {drift} current critical issue(s), {fixed} auto-fixed, {remaining} remaining for manual handling.",
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
      unitId: "Unit",
      migrateSourcePath: "Migrated Directory",
      rollbackSourcePath: "Rolled Back Directory",
      targetPath: "External Directory",
      pathUnavailable: "Path info unavailable",
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
