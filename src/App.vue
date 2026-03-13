<script setup lang="ts">
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { confirm as dialogConfirm } from "@tauri-apps/plugin-dialog";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  Activity,
  FileText,
  HardDrive,
  LayoutGrid,
  Monitor
} from "lucide-vue-next";
import AppListView from "./components/AppListView.vue";
import HealthPanelView from "./components/HealthPanelView.vue";
import MigrationDialog from "./components/MigrationDialog.vue";
import OperationLogExportView from "./components/OperationLogExportView.vue";
import type {
  AppScanResult,
  DiskStatus,
  RelocationSummary,
  RollbackRequest
} from "./types/contracts";
import { useI18n } from "./i18n";
import { formatCommandError } from "./utils/error";

interface AppCard {
  id: string;
  name: string;
  icon: string;
  iconPath: string | null;
  size: string;
  sizeLabel: string;
  isMigrated: boolean;
  targetDisk: string | null;
  path: string;
  paths: string[];
  pathGroups: {
    key: string;
    label: string;
    paths: string[];
    entries: {
      path: string;
      displayName: string;
      migrated: boolean;
      pending: boolean;
    }[];
  }[];
  pendingPathCount: number;
  migratedPathCount: number;
  desc: string;
  availability: AppScanResult["availability"];
  blockedReason: string | null;
  requiresConfirmation: boolean;
  hasExecutableUnit: boolean;
  running: boolean;
}

const activeTab = ref("apps");
const apps = ref<AppScanResult[]>([]);
const disks = ref<DiskStatus[]>([]);
const systemDisk = ref<DiskStatus | null>(null);
const relocations = ref<RelocationSummary[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
const info = ref<string | null>(null);
let infoTimer: number | null = null;

const showModal = ref(false);
const selectedAppId = ref("");
const { locale, setLocale, t } = useI18n();

const iconMap: Record<string, string> = {
  "wechat-non-mas": "💬",
  "telegram-mac-native": "✈️",
  "jetbrains-caches": "🧠",
  "xcode-derived-data": "🛠️",
  "mas-sandbox-containers": "🧩",
  "docker-desktop-data-root": "🐳"
};

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function createBatchTraceId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return `tr_batch_${crypto.randomUUID()}`;
  }
  return `tr_batch_${Date.now()}_${Math.random().toString(36).slice(2, 10)}`;
}

function parseDiskName(targetPath: string | undefined): string | null {
  if (!targetPath) {
    return null;
  }
  const match = targetPath.match(/^\/Volumes\/([^/]+)/);
  if (!match) {
    return null;
  }
  return decodeURIComponent(match[1]);
}

function isExecutablePath(path: AppScanResult["detected_paths"][number]): boolean {
  const blocked = path.blocked_reason?.trim();
  return path.enabled !== false && !blocked;
}

function pathRequiresConfirmation(path: AppScanResult["detected_paths"][number]): boolean {
  if (path.requires_confirmation === true) {
    return true;
  }
  const risk = (path.risk_level ?? "stable").toString().toLowerCase();
  return risk !== "stable";
}

function pathNeedsMigration(path: AppScanResult["detected_paths"][number]): boolean {
  if (!isExecutablePath(path)) {
    return false;
  }
  if (path.exists) {
    return !path.is_symlink;
  }
  return path.allow_bootstrap_if_source_missing === true;
}

function isActiveRelocationState(state: string | undefined): boolean {
  const normalized = (state ?? "").trim().toUpperCase();
  return normalized === "HEALTHY" || normalized === "DEGRADED" || normalized === "BROKEN";
}

function pathGroupKeyFromUnitId(unitId: string | undefined): string | null {
  const normalized = unitId?.trim();
  if (!normalized) {
    return null;
  }
  const segments = normalized
    .split("::")
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0);
  if (segments.length <= 1) {
    return null;
  }
  return segments[1];
}

function buildPathGroups(paths: AppScanResult["detected_paths"]): {
  key: string;
  label: string;
  paths: string[];
  entries: {
    path: string;
    displayName: string;
    migrated: boolean;
    pending: boolean;
  }[];
}[] {
  const grouped = new Map<
    string,
    Map<
      string,
      {
        path: string;
        displayName: string;
        migrated: boolean;
        pending: boolean;
      }
    >
  >();
  for (const item of paths) {
    const sourcePath = (item.path ?? "").trim();
    if (!sourcePath) {
      continue;
    }
    const matchGroup = pathGroupKeyFromUnitId(item.unit_id);
    const key = matchGroup && matchGroup.length > 0 ? matchGroup : "__default__";
    const groupEntries = grouped.get(key) ?? new Map();
    const current = groupEntries.get(sourcePath);
    const displayName = (item.display_name ?? "").trim();
    const next = {
      path: sourcePath,
      displayName,
      migrated: item.exists && item.is_symlink,
      pending: pathNeedsMigration(item)
    };
    if (!current) {
      groupEntries.set(sourcePath, next);
    } else {
      const pending = current.pending || next.pending;
      groupEntries.set(sourcePath, {
        path: sourcePath,
        displayName: current.displayName || next.displayName,
        pending,
        migrated: pending ? false : current.migrated || next.migrated
      });
    }
    grouped.set(key, groupEntries);
  }

  const result = Array.from(grouped.entries()).map(([key, values]) => {
    const entries = Array.from(values.values()).sort((left, right) =>
      left.path.localeCompare(right.path)
    );
    return {
      key,
      label:
        key === "__default__"
          ? t("appList.pathGroup.default")
          : t("appList.pathGroup.account", { account: key }),
      paths: entries.map((entry) => entry.path),
      entries
    };
  });

  result.sort((left, right) => {
    if (left.key === "__default__") {
      return -1;
    }
    if (right.key === "__default__") {
      return 1;
    }
    return left.label.localeCompare(right.label);
  });
  return result;
}

const appCards = computed<AppCard[]>(() => {
  const latestRelocation = new Map<string, RelocationSummary>();
  const relocationSavedBytes = new Map<string, number>();
  for (const row of relocations.value) {
    if (!latestRelocation.has(row.app_id)) {
      latestRelocation.set(row.app_id, row);
    }
    if (!isActiveRelocationState(row.state)) {
      continue;
    }
    const rowBytes =
      (typeof row.source_size_bytes === "number" && row.source_size_bytes > 0
        ? row.source_size_bytes
        : 0) ||
      (typeof row.target_size_bytes === "number" && row.target_size_bytes > 0
        ? row.target_size_bytes
        : 0);
    if (rowBytes <= 0) {
      continue;
    }
    relocationSavedBytes.set(row.app_id, (relocationSavedBytes.get(row.app_id) ?? 0) + rowBytes);
  }

  const cards: AppCard[] = [];

  for (const app of apps.value) {
    const executablePaths = app.detected_paths.filter((path) => isExecutablePath(path));
    const hasExecutableUnit = executablePaths.length > 0;
    const consideredPaths = hasExecutableUnit ? executablePaths : app.detected_paths;
    const pathGroups = buildPathGroups(consideredPaths);
    const paths = pathGroups.flatMap((group) => group.paths);
    const pathEntries = pathGroups.flatMap((group) => group.entries);
    const primaryPath = paths[0] ?? t("app.pathFallback");
    const existingPaths = consideredPaths.filter((path) => path.exists);
    const scannedSizeBytes = existingPaths.reduce((sum, path) => sum + path.size_bytes, 0);
    const hasScannedPaths = existingPaths.length > 0;
    const isMigrated = hasExecutableUnit
      ? executablePaths.every((path) => path.exists && path.is_symlink)
      : existingPaths.some((path) => path.is_symlink);
    const requiresConfirmation = hasExecutableUnit
      ? executablePaths.some((path) => pathRequiresConfirmation(path))
      : false;
    const blockedReason = app.blocked_reason?.trim() || null;
    const activeLocale = locale.value.toLowerCase();
    const localeBase = activeLocale.split("-")[0];
    const localizedDescription =
      app.description_i18n?.[activeLocale] ?? app.description_i18n?.[localeBase] ?? null;
    const description = localizedDescription?.trim() || null;
    const relocation = latestRelocation.get(app.app_id);
    const estimatedSavedBytes = relocationSavedBytes.get(app.app_id) ?? 0;
    const sizeBytes = hasScannedPaths ? scannedSizeBytes : isMigrated ? estimatedSavedBytes : 0;
    const sizeLabel = isMigrated ? t("appList.sizeLabelSaved") : t("appList.sizeLabelCurrent");
    const pendingPathCount = pathEntries.filter((entry) => entry.pending).length;
    const migratedPathCount = pathEntries.filter((entry) => entry.migrated).length;

    cards.push({
      id: app.app_id,
      name: app.display_name,
      icon: iconMap[app.app_id] ?? "📦",
      iconPath: app.icon_data_url ?? (app.icon_path ? convertFileSrc(app.icon_path) : null),
      size: formatBytes(sizeBytes),
      sizeLabel,
      isMigrated,
      targetDisk: parseDiskName(relocation?.target_path),
      path: primaryPath,
      paths,
      pathGroups,
      pendingPathCount,
      migratedPathCount,
      desc: description ?? t("app.descFallback"),
      availability: app.availability,
      blockedReason,
      requiresConfirmation,
      hasExecutableUnit,
      running: app.running
    });
  }

  return cards;
});

const selectedApp = computed(() =>
  apps.value.find((app) => app.app_id === selectedAppId.value) ?? null
);
const appDisplayNames = computed<Record<string, string>>(() => {
  return apps.value.reduce<Record<string, string>>((acc, app) => {
    acc[app.app_id] = app.display_name;
    return acc;
  }, {});
});
const systemDiskUsedPercent = computed(() => {
  const total = systemDisk.value?.total_bytes ?? 0;
  const free = systemDisk.value?.free_bytes ?? 0;
  if (!Number.isFinite(total) || total <= 0) {
    return 0;
  }
  const used = Math.max(0, total - Math.max(0, free));
  return Math.min(100, Math.max(0, (used / total) * 100));
});
const systemDiskFreeText = computed(() => formatBytes(systemDisk.value?.free_bytes ?? 0));
const systemDiskTotalText = computed(() => formatBytes(systemDisk.value?.total_bytes ?? 0));

const sidebarItems = computed(() => [
  { key: "apps", label: t("app.sidebar.apps"), icon: LayoutGrid },
  { key: "health", label: t("app.sidebar.health"), icon: Activity },
  { key: "logs", label: t("app.sidebar.logs"), icon: FileText }
]);

async function refreshAll(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const corePromise = Promise.all([
      invoke<AppScanResult[]>("scan_apps"),
      invoke<DiskStatus[]>("get_disk_status"),
      invoke<RelocationSummary[]>("list_relocations")
    ]);
    const systemDiskPromise = invoke<DiskStatus>("get_system_disk_status").catch(() => null);
    const [[nextApps, nextDisks, nextRelocations], nextSystemDisk] = await Promise.all([
      corePromise,
      systemDiskPromise
    ]);
    apps.value = nextApps;
    disks.value = nextDisks;
    relocations.value = nextRelocations;
    systemDisk.value = nextSystemDisk;
  } catch (err) {
    error.value = t("app.messages.loadFailed", { error: formatCommandError(err) });
  } finally {
    loading.value = false;
  }
}

function stopInfoTimer(): void {
  if (infoTimer !== null) {
    window.clearTimeout(infoTimer);
    infoTimer = null;
  }
}

function clearInfo(): void {
  stopInfoTimer();
  info.value = null;
}

function showInfo(message: string): void {
  info.value = message;
  stopInfoTimer();
  infoTimer = window.setTimeout(() => {
    info.value = null;
    infoTimer = null;
  }, 6000);
}

function handleMigrateClick(appId: string): void {
  selectedAppId.value = appId;
  showModal.value = true;
  clearInfo();
}

async function confirmRestore(name: string): Promise<boolean> {
  const message = t("app.messages.restoreConfirm", { name });
  try {
    return await dialogConfirm(message, {
      kind: "warning",
      okLabel: t("common.confirm"),
      cancelLabel: t("common.cancelled")
    });
  } catch {
    return window.confirm(message);
  }
}

async function handleRestore(appId: string): Promise<void> {
  const targetCard = appCards.value.find((item) => item.id === appId);
  if (!targetCard) {
    return;
  }

  if (targetCard.running) {
    error.value = t("app.messages.restoreBlockedRunning", { name: targetCard.name });
    return;
  }

  if (!(await confirmRestore(targetCard.name))) {
    return;
  }

  const rollbackTargets = relocations.value.filter(
    (item) => item.app_id === appId && isActiveRelocationState(item.state)
  );
  if (rollbackTargets.length === 0) {
    error.value = t("app.messages.rollbackRecordMissing", { name: targetCard.name });
    return;
  }

  error.value = null;
  clearInfo();
  try {
    const rollbackTraceId = createBatchTraceId();
    for (const target of rollbackTargets) {
      const req: RollbackRequest = {
        relocation_id: target.relocation_id,
        force: true,
        trace_id: rollbackTraceId
      };
      await invoke("rollback_relocation", { req });
    }
    showInfo(t("app.messages.restoreDone", { name: targetCard.name }));
    await refreshAll();
  } catch (err) {
    error.value = t("app.messages.restoreFailed", { error: formatCommandError(err) });
  }
}

function handleMigrationDone(message: string): void {
  showModal.value = false;
  selectedAppId.value = "";
  showInfo(message || t("app.messages.migrationDone", { label: "-" }));
  void refreshAll();
}

function handleModalClose(): void {
  showModal.value = false;
  selectedAppId.value = "";
}

onMounted(() => {
  void refreshAll();
});

onBeforeUnmount(() => {
  stopInfoTimer();
});
</script>

<template>
  <div class="flex h-screen bg-white font-sans overflow-hidden">
    <div class="w-64 bg-[#f6f6f6] border-r border-gray-200 flex flex-col h-full select-none">
      <div class="p-6 pb-2">
        <h1 class="text-xl font-bold text-gray-800 flex items-center gap-2">
          <HardDrive class="text-blue-500" :size="24" />
          Disk Relocator
        </h1>
        <p class="text-xs text-gray-500 mt-1">{{ t("app.subtitle") }}</p>
      </div>

      <div class="flex-1 overflow-y-auto px-3 py-4 space-y-1">
        <button
          v-for="item in sidebarItems"
          :key="item.key"
          @click="activeTab = item.key"
          :class="[
            'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors',
            activeTab === item.key
              ? 'bg-blue-50 text-blue-600 shadow-sm border border-blue-100/50'
              : 'text-gray-600 hover:bg-gray-200/50'
          ]"
        >
          <component
            :is="item.icon"
            :size="18"
            :class="activeTab === item.key ? 'text-blue-500' : 'text-gray-500'"
          />
          {{ item.label }}
        </button>
      </div>

      <div class="p-4 m-3 bg-white rounded-xl shadow-sm border border-gray-100">
        <div class="flex items-center gap-2 mb-2">
          <Monitor :size="16" class="text-gray-500" />
          <span class="text-sm font-medium text-gray-700">{{ t("app.systemDisk.title") }}</span>
        </div>
        <div v-if="systemDisk && systemDisk.total_bytes > 0">
          <div class="w-full bg-gray-100 rounded-full h-2 mb-1">
            <div
              class="bg-blue-500 h-2 rounded-full transition-all duration-500"
              :style="{ width: `${systemDiskUsedPercent.toFixed(1)}%` }"
            ></div>
          </div>
          <div class="flex justify-between text-xs text-gray-500">
            <span>{{ t("app.systemDisk.free", { free: systemDiskFreeText }) }}</span>
            <span>{{ t("app.systemDisk.total", { total: systemDiskTotalText }) }}</span>
          </div>
        </div>
        <div v-else class="text-xs text-gray-500 leading-5">
          {{ t("app.systemDisk.unavailable") }}
        </div>
        <div class="mt-3 pt-3 border-t border-gray-100 flex items-center justify-between">
          <span class="text-xs text-gray-500">{{ t("common.language") }}</span>
          <div class="flex items-center gap-1">
            <button
              type="button"
              class="px-2 py-0.5 text-xs rounded border border-gray-200"
              :class="locale === 'zh' ? 'bg-blue-50 text-blue-600 border-blue-200' : 'text-gray-600'"
              @click="setLocale('zh')"
            >
              {{ t("common.zh") }}
            </button>
            <button
              type="button"
              class="px-2 py-0.5 text-xs rounded border border-gray-200"
              :class="locale === 'en' ? 'bg-blue-50 text-blue-600 border-blue-200' : 'text-gray-600'"
              @click="setLocale('en')"
            >
              {{ t("common.en") }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <main class="flex-1 flex flex-col h-full bg-[#fcfcfc] overflow-y-auto">
      <AppListView
        v-if="activeTab === 'apps'"
        :apps="appCards"
        :loading="loading"
        :error="error"
        @refresh="refreshAll"
        @migrate="handleMigrateClick"
        @restore="handleRestore"
      />
      <HealthPanelView v-else-if="activeTab === 'health'" :app-display-names="appDisplayNames" />
      <OperationLogExportView v-else-if="activeTab === 'logs'" />

      <div
        v-if="info"
        class="mx-8 mb-4 rounded-xl border border-green-200 bg-green-50 px-4 py-3 text-sm text-green-700 flex items-center justify-between gap-3"
      >
        <span>{{ info }}</span>
        <button
          type="button"
          data-test="clear-info-btn"
          class="px-2 py-0.5 rounded border border-green-300 text-green-700 hover:bg-green-100"
          @click="clearInfo"
        >
          {{ t("common.close") }}
        </button>
      </div>
    </main>

    <MigrationDialog
      :show-modal="showModal"
      :selected-app-id="selectedAppId"
      :selected-app="selectedApp"
      :disks="disks"
      @close="handleModalClose"
      @done="handleMigrationDone"
    />
  </div>
</template>
