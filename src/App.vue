<script setup lang="ts">
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { confirm as dialogConfirm } from "@tauri-apps/plugin-dialog";
import { computed, onMounted, ref } from "vue";
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

function isActiveRelocationState(state: string | undefined): boolean {
  const normalized = (state ?? "").trim().toUpperCase();
  return normalized === "HEALTHY" || normalized === "DEGRADED" || normalized === "BROKEN";
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
    const existingPaths = consideredPaths.filter((path) => path.exists);
    const scannedSizeBytes = existingPaths.reduce((sum, path) => sum + path.size_bytes, 0);
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
    const sizeBytes = scannedSizeBytes > 0 ? scannedSizeBytes : isMigrated ? estimatedSavedBytes : 0;
    const sizeLabel = isMigrated ? t("appList.sizeLabelSaved") : t("appList.sizeLabelCurrent");

    cards.push({
      id: app.app_id,
      name: app.display_name,
      icon: iconMap[app.app_id] ?? "📦",
      iconPath: app.icon_data_url ?? (app.icon_path ? convertFileSrc(app.icon_path) : null),
      size: formatBytes(sizeBytes),
      sizeLabel,
      isMigrated,
      targetDisk: parseDiskName(relocation?.target_path),
      path: consideredPaths[0]?.path ?? app.detected_paths[0]?.path ?? t("app.pathFallback"),
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

function handleMigrateClick(appId: string): void {
  selectedAppId.value = appId;
  showModal.value = true;
  info.value = null;
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

  if (!(await confirmRestore(targetCard.name))) {
    return;
  }

  const rollbackTarget = relocations.value.find((item) => item.app_id === appId);
  if (!rollbackTarget) {
    error.value = t("app.messages.rollbackRecordMissing", { name: targetCard.name });
    return;
  }

  error.value = null;
  info.value = null;
  try {
    const req: RollbackRequest = { relocation_id: rollbackTarget.relocation_id, force: true };
    await invoke("rollback_relocation", { req });
    info.value = t("app.messages.restoreDone", { name: targetCard.name });
    await refreshAll();
  } catch (err) {
    error.value = t("app.messages.restoreFailed", { error: formatCommandError(err) });
  }
}

function handleMigrationDone(message: string): void {
  showModal.value = false;
  selectedAppId.value = "";
  info.value = message || t("app.messages.migrationDone", { label: "-" });
  void refreshAll();
}

function handleModalClose(): void {
  showModal.value = false;
  selectedAppId.value = "";
}

onMounted(() => {
  void refreshAll();
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
      <HealthPanelView v-else-if="activeTab === 'health'" />
      <OperationLogExportView v-else-if="activeTab === 'logs'" />

      <div
        v-if="info"
        class="mx-8 mb-4 rounded-xl border border-green-200 bg-green-50 px-4 py-3 text-sm text-green-700"
      >
        {{ info }}
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
