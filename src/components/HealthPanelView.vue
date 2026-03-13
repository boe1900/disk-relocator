<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onBeforeUnmount, onMounted, computed, ref, watch } from "vue";
import { AlertCircle, CheckCircle2, ChevronLeft, ChevronRight, HardDrive } from "lucide-vue-next";
import { useI18n } from "../i18n";
import { formatCommandError } from "../utils/error";
import type {
  AppScanResult,
  DiskStatus,
  HealthEvent,
  HealthEventsRequest,
  HealthStatus,
  ReconcileRequest,
  ReconcileResult,
  RelocationSummary
} from "../types/contracts";

const props = withDefaults(
  defineProps<{
    appDisplayNames?: Record<string, string>;
  }>(),
  {
    appDisplayNames: () => ({})
  }
);

const diskPayload = ref<DiskStatus[]>([]);
const appScanPayload = ref<AppScanResult[]>([]);
const healthPayload = ref<HealthStatus[]>([]);
const historyPayload = ref<HealthEvent[]>([]);
const relocationPayload = ref<RelocationSummary[]>([]);
const selfChecking = ref(false);
const loading = ref(false);
const error = ref<string | null>(null);
const info = ref<string | null>(null);
const pathActionError = ref<string | null>(null);
const copiedPathKey = ref<string | null>(null);
const mountedDiskIndex = ref(0);
const { t } = useI18n();
let refreshTimer: number | undefined;
let copyFeedbackTimer: number | undefined;

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

const healthyCount = computed(() =>
  healthPayload.value.filter((status) => status.state === "healthy").length
);
const unhealthyCount = computed(() =>
  healthPayload.value.filter((status) => status.state !== "healthy").length
);
const mountedDisks = computed(() =>
  diskPayload.value.filter((disk) => disk.is_mounted && disk.is_writable)
);
const currentMountedDisk = computed(() => {
  if (mountedDisks.value.length === 0) {
    return null;
  }
  const index =
    ((mountedDiskIndex.value % mountedDisks.value.length) + mountedDisks.value.length) %
    mountedDisks.value.length;
  return mountedDisks.value[index];
});
const mountedDiskPosition = computed(() => {
  if (mountedDisks.value.length === 0) {
    return 0;
  }
  return (
    ((mountedDiskIndex.value % mountedDisks.value.length) + mountedDisks.value.length) %
      mountedDisks.value.length +
    1
  );
});
const relocationById = computed(() => {
  const map = new Map<string, RelocationSummary>();
  for (const row of relocationPayload.value) {
    map.set(row.relocation_id, row);
  }
  return map;
});
const sourcePathMetadata = computed(() => {
  const map = new Map<
    string,
    {
      displayName: string;
      account: string | null;
    }
  >();

  for (const app of appScanPayload.value) {
    for (const path of app.detected_paths) {
      const sourcePath = normalizePath(path.path);
      if (!hasActionablePath(sourcePath) || map.has(sourcePath)) {
        continue;
      }
      const account = accountFromUnitId(path.unit_id);
      const normalizedName = normalizedDisplayName(path.display_name, account);
      const fallbackName = app.display_name?.trim() || app.app_id;
      map.set(sourcePath, {
        displayName: normalizedName || fallbackName,
        account
      });
    }
  }

  return map;
});
const groupedHealth = computed(() => {
  const grouped = new Map<
    string,
    {
      appId: string;
      displayName: string;
      statuses: HealthStatus[];
    }
  >();

  for (const status of healthPayload.value) {
    const appId = status.app_id;
    const displayName = appDisplayName(appId);
    const entry = grouped.get(appId) ?? { appId, displayName, statuses: [] };
    entry.statuses.push(status);
    grouped.set(appId, entry);
  }

  const items = Array.from(grouped.values());
  items.sort(
    (left, right) =>
      left.displayName.localeCompare(right.displayName) || left.appId.localeCompare(right.appId)
  );
  for (const group of items) {
    group.statuses.sort((left, right) => left.relocation_id.localeCompare(right.relocation_id));
  }
  return items;
});

function appDisplayName(appId: string): string {
  const name = props.appDisplayNames?.[appId]?.trim();
  return name && name.length > 0 ? name : appId;
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function accountFromUnitId(unitId: string | undefined): string | null {
  const normalized = (unitId ?? "").trim();
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

function normalizedDisplayName(rawDisplayName: string | undefined, account: string | null): string {
  const displayName = (rawDisplayName ?? "").trim();
  if (!displayName || !account) {
    return displayName;
  }
  const suffixPattern = new RegExp(`\\s*\\[${escapeRegExp(account)}\\]\\s*$`);
  return displayName.replace(suffixPattern, "").trim();
}

function sourceMetaFor(status: HealthStatus): { displayName: string; account: string | null } {
  const sourcePath = normalizePath(sourcePathFor(status));
  const matched = sourcePathMetadata.value.get(sourcePath);
  if (matched) {
    return matched;
  }
  return {
    displayName: appDisplayName(status.app_id),
    account: null
  };
}

function sourceDisplayName(status: HealthStatus): string {
  return sourceMetaFor(status).displayName;
}

function sourceAccount(status: HealthStatus): string | null {
  return sourceMetaFor(status).account;
}

function sourceAccountLabel(status: HealthStatus): string {
  const account = sourceAccount(status);
  if (!account) {
    return "";
  }
  return t("health.userLabel", { account });
}

function healthLabel(state: HealthStatus["state"]): string {
  if (state === "healthy") {
    return t("health.state.healthy");
  }
  if (state === "degraded") {
    return t("health.state.degraded");
  }
  return t("health.state.broken");
}

function diskState(status: HealthStatus): string {
  const code = status.checks[0]?.code ?? "";
  if (code.includes("DISK_OFFLINE")) {
    return t("health.state.unmounted");
  }
  if (code.includes("TARGET_READONLY")) {
    return t("health.state.readonly");
  }
  return t("health.state.mounted");
}

function shouldShowDiskStateBadge(status: HealthStatus): boolean {
  return diskState(status) !== t("health.state.mounted");
}

function sourcePathFor(status: HealthStatus): string {
  return relocationById.value.get(status.relocation_id)?.source_path ?? "";
}

function normalizePath(rawPath: string): string {
  return rawPath.trim();
}

function hasActionablePath(rawPath: string): boolean {
  return normalizePath(rawPath).startsWith("/");
}

function makePathCopyKey(relocationId: string, kind: "source"): string {
  return `${relocationId}::${kind}`;
}

function isPathCopied(relocationId: string, kind: "source"): boolean {
  return copiedPathKey.value === makePathCopyKey(relocationId, kind);
}

function stopCopyFeedbackTimer(): void {
  if (copyFeedbackTimer !== undefined) {
    window.clearTimeout(copyFeedbackTimer);
    copyFeedbackTimer = undefined;
  }
}

async function onCopyPath(
  relocationId: string,
  kind: "source",
  rawPath: string
): Promise<void> {
  const path = normalizePath(rawPath);
  if (!hasActionablePath(path)) {
    return;
  }

  try {
    if (!navigator.clipboard?.writeText) {
      throw new Error("clipboard API unavailable");
    }
    await navigator.clipboard.writeText(path);
    copiedPathKey.value = makePathCopyKey(relocationId, kind);
    pathActionError.value = null;
    stopCopyFeedbackTimer();
    copyFeedbackTimer = window.setTimeout(() => {
      copiedPathKey.value = null;
      copyFeedbackTimer = undefined;
    }, 1200);
  } catch (err) {
    pathActionError.value = t("health.pathActions.copyFailed", {
      error: formatCommandError(err)
    });
  }
}

async function onOpenInFinder(rawPath: string): Promise<void> {
  const path = normalizePath(rawPath);
  if (!hasActionablePath(path)) {
    return;
  }

  try {
    await invoke("open_in_finder", { path });
    pathActionError.value = null;
  } catch (err) {
    pathActionError.value = t("health.pathActions.openFailed", {
      error: formatCommandError(err)
    });
  }
}

function prevMountedDisk(): void {
  if (mountedDisks.value.length <= 1) {
    return;
  }
  mountedDiskIndex.value =
    (mountedDiskIndex.value - 1 + mountedDisks.value.length) % mountedDisks.value.length;
}

function nextMountedDisk(): void {
  if (mountedDisks.value.length <= 1) {
    return;
  }
  mountedDiskIndex.value = (mountedDiskIndex.value + 1) % mountedDisks.value.length;
}

async function refreshPanel(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const req: HealthEventsRequest = { limit: 30 };
    const [disk, scans, health, history, relocations] = await Promise.all([
      invoke<DiskStatus[]>("get_disk_status"),
      invoke<AppScanResult[]>("scan_apps"),
      invoke<HealthStatus[]>("check_health"),
      invoke<HealthEvent[]>("list_health_events", { req }),
      invoke<RelocationSummary[]>("list_relocations")
    ]);
    diskPayload.value = disk;
    appScanPayload.value = scans;
    healthPayload.value = health;
    historyPayload.value = history;
    relocationPayload.value = relocations;
  } catch (err) {
    error.value = t("health.errorRefreshFailed", { error: formatCommandError(err) });
  } finally {
    loading.value = false;
  }
}

async function runReconcile(applySafeFixes: boolean): Promise<ReconcileResult> {
  const req: ReconcileRequest = {
    apply_safe_fixes: applySafeFixes,
    limit: 500
  };
  return invoke<ReconcileResult>("reconcile_relocations", { req });
}

async function onSelfCheck(): Promise<void> {
  selfChecking.value = true;
  error.value = null;
  try {
    const result = await runReconcile(true);
    await refreshPanel();
    const activeRelocationIds = new Set(healthPayload.value.map((status) => status.relocation_id));
    const currentCriticalIssues = result.issues.filter(
      (issue) => issue.severity === "critical" && activeRelocationIds.has(issue.relocation_id)
    );
    const fixed = currentCriticalIssues.filter((issue) => issue.safe_fix_applied).length;
    const remaining = currentCriticalIssues.length - fixed;
    info.value = t("health.infoSelfCheckDone", {
      drift: currentCriticalIssues.length,
      fixed,
      remaining
    });
  } catch (err) {
    error.value = t("health.errorSelfCheckFailed", { error: formatCommandError(err) });
  } finally {
    selfChecking.value = false;
  }
}

onMounted(async () => {
  await refreshPanel();
  try {
    await runReconcile(true);
    await refreshPanel();
  } catch {
    // Keep health panel available even if reconcile scan fails on initial load.
  }
  refreshTimer = window.setInterval(() => {
    void refreshPanel();
  }, 10000);
});

watch(
  () => mountedDisks.value.length,
  (nextLength) => {
    if (nextLength === 0) {
      mountedDiskIndex.value = 0;
      return;
    }
    if (mountedDiskIndex.value >= nextLength) {
      mountedDiskIndex.value = nextLength - 1;
    }
    if (mountedDiskIndex.value < 0) {
      mountedDiskIndex.value = 0;
    }
  }
);

onBeforeUnmount(() => {
  if (refreshTimer !== undefined) {
    window.clearInterval(refreshTimer);
  }
  stopCopyFeedbackTimer();
});
</script>

<template>
  <div class="p-8 max-w-5xl mx-auto animation-fade-in w-full">
    <div class="mb-8 flex items-start justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-gray-900">{{ t("health.title") }}</h2>
        <p class="text-gray-500 mt-2">{{ t("health.subtitle") }}</p>
      </div>
      <button
        type="button"
        :disabled="loading || selfChecking"
        @click="onSelfCheck"
        class="bg-gray-900 hover:bg-black text-white px-4 py-2 rounded-lg text-sm"
      >
        {{ selfChecking ? t("health.checking") : t("health.checkNow") }}
      </button>
    </div>

    <div class="grid grid-cols-2 gap-6 mb-8">
      <div class="bg-white p-6 rounded-2xl border border-green-200 bg-green-50/30">
        <div class="flex items-center gap-3 mb-2">
          <CheckCircle2 class="text-green-500" :size="24" />
          <h3 class="text-lg font-semibold text-green-900">
            {{
              unhealthyCount === 0
                ? t("health.allHealthy")
                : t("health.issueCount", { count: unhealthyCount })
            }}
          </h3>
        </div>
        <p class="text-sm text-green-700">
          {{ t("health.summary", { total: healthPayload.length, healthy: healthyCount }) }}
        </p>
      </div>

      <div class="bg-white p-6 rounded-2xl border border-gray-200 shadow-sm">
        <div class="flex items-center justify-between gap-3 mb-2">
          <div class="flex items-center gap-3 min-w-0">
            <HardDrive class="text-blue-500 flex-shrink-0" :size="24" />
            <h3 class="text-lg font-semibold text-gray-800 truncate">
              {{
                mountedDisks.length > 0
                  ? t("health.disksOnlineCount", { count: mountedDisks.length })
                  : t("health.noDisk")
              }}
            </h3>
          </div>
          <div v-if="mountedDisks.length > 1" class="flex items-center gap-1 text-xs text-gray-500">
            <button
              type="button"
              class="p-1 rounded border border-gray-200 hover:bg-gray-100"
              @click="prevMountedDisk"
            >
              <ChevronLeft :size="14" />
            </button>
            <span class="font-mono">{{ mountedDiskPosition }} / {{ mountedDisks.length }}</span>
            <button
              type="button"
              class="p-1 rounded border border-gray-200 hover:bg-gray-100"
              @click="nextMountedDisk"
            >
              <ChevronRight :size="14" />
            </button>
          </div>
        </div>
        <div class="text-xs text-gray-500 mb-2">{{ t("health.diskFilterHint") }}</div>
        <div v-if="currentMountedDisk" class="space-y-1.5">
          <div class="text-sm text-gray-700 truncate">{{ currentMountedDisk.display_name }}</div>
          <div class="text-xs text-gray-500 font-mono truncate">{{ currentMountedDisk.mount_point }}</div>
          <div class="text-sm text-gray-500">
            {{
              t("health.diskCapacity", {
                free: formatBytes(currentMountedDisk.free_bytes),
                total: formatBytes(currentMountedDisk.total_bytes)
              })
            }}
          </div>
        </div>
        <p class="text-sm text-gray-500" v-else>{{ t("health.mountPointHint") }}</p>
      </div>
    </div>

    <div
      v-if="pathActionError"
      class="mb-4 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
    >
      {{ pathActionError }}
    </div>

    <h3 class="text-lg font-semibold text-gray-800 mb-4">{{ t("health.tableTitle") }}</h3>
    <div class="space-y-4">
      <div
        v-for="group in groupedHealth"
        :key="group.appId"
        class="bg-white rounded-2xl border border-gray-200 shadow-sm overflow-hidden"
      >
        <div class="px-4 py-3 border-b border-gray-100 bg-gray-50/70 flex items-center justify-between gap-3">
          <div class="min-w-0">
            <div class="text-sm font-semibold text-gray-800 truncate">{{ group.displayName }}</div>
            <div class="text-xs text-gray-400 font-mono truncate">{{ group.appId }}</div>
          </div>
          <div class="text-xs text-gray-500 whitespace-nowrap">
            {{ t("health.groupCount", { count: group.statuses.length }) }}
          </div>
        </div>

        <div class="divide-y divide-gray-100">
          <div
            v-for="status in group.statuses"
            :key="status.relocation_id"
            class="p-4"
          >
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center justify-between gap-2 mb-2">
                <div class="text-xs text-gray-400 font-mono truncate">{{ status.relocation_id }}</div>
                <div class="flex flex-wrap items-center gap-2">
                  <span v-if="status.state === 'healthy'" class="text-green-600 flex items-center gap-1">
                    <CheckCircle2 :size="14" /> {{ healthLabel(status.state) }}
                  </span>
                  <span v-else class="text-red-500 flex items-center gap-1">
                    <AlertCircle :size="14" /> {{ healthLabel(status.state) }}
                  </span>
                  <span
                    v-if="shouldShowDiskStateBadge(status)"
                    class="text-xs px-1.5 py-0.5 rounded border text-red-600 border-red-200 bg-red-50"
                  >
                    {{ diskState(status) }}
                  </span>
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-2">
                <span class="text-xs text-gray-600 truncate max-w-xs">{{ sourceDisplayName(status) }}</span>
                <span
                  v-if="sourceAccount(status)"
                  class="text-[11px] px-1.5 py-0.5 rounded border border-gray-200 bg-gray-50 text-gray-500"
                >
                  {{ sourceAccountLabel(status) }}
                </span>
                <template v-if="hasActionablePath(sourcePathFor(status))">
                  <button
                    type="button"
                    data-test="health-open-path-btn"
                    class="text-xs text-gray-600 hover:text-gray-800 whitespace-nowrap"
                    @click.stop.prevent="onOpenInFinder(sourcePathFor(status))"
                  >
                    {{ t("health.pathActions.openInFinder") }}
                  </button>
                  <button
                    type="button"
                    data-test="health-copy-path-btn"
                    class="text-xs text-blue-600 hover:text-blue-700 whitespace-nowrap"
                    @click.stop.prevent="onCopyPath(status.relocation_id, 'source', sourcePathFor(status))"
                  >
                    {{
                      isPathCopied(status.relocation_id, "source")
                        ? t("health.pathActions.copied")
                        : t("health.pathActions.copyPath")
                    }}
                  </button>
                </template>
                <span v-else class="text-xs text-gray-400">
                  {{ t("health.pathActions.pathUnavailable") }}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div
        v-if="!loading && groupedHealth.length === 0"
        class="bg-white rounded-2xl border border-gray-200 shadow-sm p-6 text-center text-sm text-gray-500"
      >
        {{ t("health.empty") }}
      </div>
    </div>

    <div v-if="historyPayload.length > 0" class="mt-6 rounded-2xl border border-gray-200 bg-white p-4">
      <h4 class="text-sm font-semibold text-gray-800 mb-2">{{ t("health.recentEvents") }}</h4>
      <div class="space-y-2 text-xs text-gray-600">
        <div v-for="item in historyPayload.slice(0, 6)" :key="item.snapshot_id">
          {{ item.observed_at }} · {{ item.app_id }} · {{ item.check_code }} · {{ item.state }}
        </div>
      </div>
    </div>

    <div v-if="info" class="mt-4 rounded-xl border border-green-200 bg-green-50 px-4 py-3 text-sm text-green-700">
      {{ info }}
    </div>
    <div v-if="error" class="mt-4 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
      {{ error }}
    </div>
  </div>
</template>
