<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import { useI18n } from "../i18n";
import { formatCommandError } from "../utils/error";

interface AppCard {
  id: string;
  name: string;
  icon: string;
  iconPath: string | null;
  size: string;
  sizeLabel?: string;
  isMigrated: boolean;
  targetDisk: string | null;
  path: string;
  paths?: string[];
  pathGroups?: {
    key: string;
    label: string;
    paths: string[];
    entries?: {
      path: string;
      displayName?: string;
      migrated: boolean;
      pending: boolean;
    }[];
  }[];
  pendingPathCount?: number;
  migratedPathCount?: number;
  desc: string;
  availability: "active" | "blocked" | "deprecated";
  blockedReason?: string | null;
  hasExecutableUnit?: boolean;
  running: boolean;
}

const props = defineProps<{
  apps: AppCard[];
  loading: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  (e: "refresh"): void;
  (e: "migrate", appId: string): void;
  (e: "restore", appId: string): void;
}>();
const { t } = useI18n();

const iconLoadFailed = reactive<Record<string, boolean>>({});
const pathActionError = ref<string | null>(null);
const copiedPathKey = ref<string | null>(null);
const detailAppId = ref<string | null>(null);
const activeDetailGroupKey = ref("");

let copyFeedbackTimer: number | null = null;

function migrationHint(app: AppCard): string {
  if (app.availability === "blocked") {
    const reason = app.blockedReason?.trim();
    return reason
      ? t("appList.hint.blockedWithReason", { reason })
      : t("appList.hint.blocked");
  }
  if (app.availability === "deprecated") {
    return t("appList.hint.deprecated");
  }
  if (app.hasExecutableUnit === false) {
    return t("appList.hint.noExecutableUnit");
  }
  if (app.running) {
    return t("appList.hint.running");
  }
  if (app.isMigrated) {
    return t("appList.hint.migrated");
  }
  return t("appList.hint.ready");
}

function canMigrate(app: AppCard): boolean {
  if (app.availability === "blocked" || app.availability === "deprecated") {
    return false;
  }
  if (app.hasExecutableUnit === false) {
    return false;
  }
  return !app.running && !app.isMigrated;
}

function canRestore(app: AppCard): boolean {
  if (app.running) {
    return false;
  }
  return hasRestorablePath(app);
}

function hasRestorablePath(app: AppCard): boolean {
  if (typeof app.migratedPathCount === "number" && Number.isFinite(app.migratedPathCount)) {
    return app.migratedPathCount > 0;
  }
  return app.isMigrated;
}

function markIconError(appId: string): void {
  iconLoadFailed[appId] = true;
}

function migratedLabel(app: AppCard): string {
  if (app.targetDisk) {
    return t("appList.migratedTo", { disk: app.targetDisk });
  }
  return t("appList.migrated");
}

function stopCopyFeedbackTimer(): void {
  if (copyFeedbackTimer !== null) {
    window.clearTimeout(copyFeedbackTimer);
    copyFeedbackTimer = null;
  }
}

function normalizePath(rawPath: string): string {
  return rawPath.trim();
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function normalizedDisplayName(rawDisplayName: string | undefined, groupKey: string): string {
  const displayName = (rawDisplayName ?? "").trim();
  if (!displayName || groupKey === "__default__") {
    return displayName;
  }
  const suffixPattern = new RegExp(`\\s*\\[${escapeRegExp(groupKey)}\\]\\s*$`);
  return displayName.replace(suffixPattern, "").trim();
}

function hasActionablePath(rawPath: string): boolean {
  return normalizePath(rawPath).startsWith("/");
}

function appPathGroups(app: AppCard): { key: string; label: string; paths: string[] }[] {
  if (Array.isArray(app.pathGroups) && app.pathGroups.length > 0) {
    return app.pathGroups;
  }
  const fallback = normalizePath(app.path);
  if (fallback) {
    return [
      {
        key: "__default__",
        label: t("appList.pathGroup.default"),
        paths: [fallback]
      }
    ];
  }
  return [];
}

function groupPathEntries(group: {
  key: string;
  label: string;
  paths: string[];
  entries?: {
    path: string;
    displayName?: string;
    migrated: boolean;
    pending: boolean;
  }[];
}): {
  path: string;
  displayName?: string;
  migrated: boolean;
  pending: boolean;
}[] {
  if (Array.isArray(group.entries) && group.entries.length > 0) {
    return group.entries;
  }
  return group.paths.map((path) => ({
    path,
    displayName: undefined,
    migrated: false,
    pending: false
  }));
}

function appPathCount(app: AppCard): number {
  if (Array.isArray(app.paths) && app.paths.length > 0) {
    return app.paths.length;
  }
  return appPathGroups(app).reduce((sum, group) => sum + group.paths.length, 0);
}

function appPendingPathCount(app: AppCard): number {
  if (typeof app.pendingPathCount === "number" && Number.isFinite(app.pendingPathCount)) {
    return Math.max(0, Math.floor(app.pendingPathCount));
  }
  return appPathGroups(app).reduce(
    (sum, group) => sum + groupPathEntries(group).filter((entry) => entry.pending).length,
    0
  );
}

function onOpenPathDetails(app: AppCard): void {
  detailAppId.value = app.id;
  const groups = appPathGroups(app);
  const firstMatchGroup = groups.find((group) => group.key !== "__default__")?.key;
  activeDetailGroupKey.value = firstMatchGroup ?? groups[0]?.key ?? "";
}

function onClosePathDetails(): void {
  detailAppId.value = null;
  activeDetailGroupKey.value = "";
}

const detailApp = computed(() => props.apps.find((app) => app.id === detailAppId.value) ?? null);

const detailGroups = computed(() => {
  if (!detailApp.value) {
    return [] as { key: string; label: string; paths: string[] }[];
  }
  return appPathGroups(detailApp.value);
});

const detailHasTabs = computed(
  () => detailGroups.value.filter((group) => group.key !== "__default__").length > 1
);

const detailVisibleGroups = computed(() => {
  if (!detailHasTabs.value) {
    return detailGroups.value;
  }
  const activeKey = activeDetailGroupKey.value;
  if (!activeKey) {
    return [];
  }
  return detailGroups.value.filter((group) => group.key === activeKey);
});

watch(detailGroups, (groups) => {
  if (groups.length === 0) {
    activeDetailGroupKey.value = "";
    return;
  }
  if (!detailHasTabs.value) {
    activeDetailGroupKey.value = groups[0].key;
    return;
  }
  const availableKeys = groups
    .filter((group) => group.key !== "__default__")
    .map((group) => group.key);
  if (!availableKeys.includes(activeDetailGroupKey.value)) {
    activeDetailGroupKey.value = availableKeys[0] ?? groups[0].key;
  }
});

function makePathCopyKey(appId: string, rawPath: string): string {
  return `${appId}::${normalizePath(rawPath)}`;
}

function isPathCopied(appId: string, rawPath: string): boolean {
  return copiedPathKey.value === makePathCopyKey(appId, rawPath);
}

async function onCopyPath(appId: string, rawPath: string): Promise<void> {
  const path = normalizePath(rawPath);
  if (!hasActionablePath(path)) {
    return;
  }

  try {
    if (!navigator.clipboard?.writeText) {
      throw new Error("clipboard API unavailable");
    }
    await navigator.clipboard.writeText(path);
    copiedPathKey.value = makePathCopyKey(appId, path);
    pathActionError.value = null;
    stopCopyFeedbackTimer();
    copyFeedbackTimer = window.setTimeout(() => {
      copiedPathKey.value = null;
      copyFeedbackTimer = null;
    }, 1200);
  } catch (err) {
    pathActionError.value = t("appList.pathActions.copyFailed", {
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
    pathActionError.value = t("appList.pathActions.openFailed", {
      error: formatCommandError(err)
    });
  }
}

onBeforeUnmount(() => {
  stopCopyFeedbackTimer();
});
</script>

<template>
  <div class="p-8 max-w-5xl mx-auto animation-fade-in w-full">
    <div class="mb-8 flex items-start justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-gray-900">{{ t("appList.title") }}</h2>
        <p class="text-gray-500 mt-2">{{ t("appList.subtitle") }}</p>
      </div>
      <button
        type="button"
        :disabled="props.loading"
        @click="emit('refresh')"
        class="bg-gray-900 hover:bg-black text-white px-4 py-2 rounded-lg text-sm"
      >
        {{ props.loading ? t("appList.refreshing") : t("appList.refresh") }}
      </button>
    </div>

    <div v-if="props.error" class="mb-4 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
      {{ props.error }}
    </div>
    <div v-if="pathActionError" class="mb-4 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
      {{ pathActionError }}
    </div>

    <div class="grid grid-cols-1 gap-4">
      <div
        v-for="app in props.apps"
        :key="app.id"
        class="bg-white rounded-2xl p-5 border border-gray-200 shadow-sm hover:shadow-md transition-shadow flex items-center gap-5"
      >
        <div class="w-14 h-14 bg-gray-50 rounded-2xl overflow-hidden flex items-center justify-center border border-gray-100 shadow-inner">
          <img
            v-if="app.iconPath && !iconLoadFailed[app.id]"
            :src="app.iconPath"
            :alt="app.name"
            class="w-full h-full object-cover scale-125"
            @error="markIconError(app.id)"
          />
          <span v-else class="text-3xl">{{ app.icon }}</span>
        </div>

        <div class="flex-1 min-w-0">
          <div class="flex items-center gap-2 flex-wrap">
            <h3 class="text-lg font-semibold text-gray-800">{{ app.name }}</h3>
            <span v-if="app.isMigrated" class="bg-green-100 text-green-700 text-xs px-2 py-0.5 rounded-full font-medium">
              {{ migratedLabel(app) }}
            </span>
            <span
              v-if="app.availability === 'deprecated'"
              class="bg-gray-100 text-gray-700 text-xs px-2 py-0.5 rounded-full font-medium"
            >
              {{ t("appList.status.deprecated") }}
            </span>
            <span
              v-else-if="app.availability === 'blocked'"
              class="bg-red-100 text-red-700 text-xs px-2 py-0.5 rounded-full font-medium"
            >
              {{ t("appList.status.blocked") }}
            </span>
          </div>
          <p class="text-sm text-gray-500 mt-1">{{ app.desc }}</p>
          <p class="text-xs text-gray-400 mt-1">{{ migrationHint(app) }}</p>
          <div v-if="appPathCount(app) > 0" class="mt-1 flex items-center gap-2">
            <span class="text-xs text-gray-400">{{ t("appList.pathCount", { count: appPathCount(app) }) }}</span>
            <span
              v-if="appPendingPathCount(app) > 0"
              class="text-[11px] px-1.5 py-0.5 rounded-full bg-amber-100 text-amber-700 border border-amber-200"
            >
              {{ t("appList.pathActions.pendingBadge", { count: appPendingPathCount(app) }) }}
            </span>
            <button
              type="button"
              data-test="app-path-details-btn"
              :disabled="props.loading || appPathGroups(app).length === 0"
              @click.stop.prevent="onOpenPathDetails(app)"
              class="text-xs text-blue-600 hover:text-blue-700 disabled:text-gray-400"
            >
              {{ t("appList.pathActions.viewDetails") }}
            </button>
          </div>
        </div>

        <div class="text-right px-4">
          <div class="text-lg font-bold text-gray-700">{{ app.size }}</div>
          <div class="text-xs text-gray-400">{{ app.sizeLabel || t("appList.sizeLabelCurrent") }}</div>
        </div>

        <div class="flex-shrink-0 border-l border-gray-100 pl-5">
          <div class="flex flex-col gap-2">
            <button
              v-if="!app.isMigrated"
              type="button"
              :disabled="!canMigrate(app)"
              @click="emit('migrate', app.id)"
              class="bg-blue-500 hover:bg-blue-600 disabled:bg-gray-300 text-white px-5 py-2 rounded-lg font-medium transition-colors"
            >
              {{ t("appList.migrate") }}
            </button>
            <button
              v-if="hasRestorablePath(app)"
              type="button"
              :disabled="!canRestore(app)"
              @click="emit('restore', app.id)"
              class="bg-gray-100 hover:bg-gray-200 disabled:bg-gray-100 disabled:text-gray-400 text-gray-700 px-5 py-2 rounded-lg font-medium transition-colors"
            >
              {{ t("appList.restore") }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <div
      v-if="detailApp"
      data-test="app-path-details-modal"
      class="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50 p-4 overflow-y-auto"
    >
      <div class="bg-white rounded-2xl shadow-2xl w-full max-w-3xl max-h-[92vh] overflow-hidden flex flex-col my-auto">
        <div class="p-6 border-b border-gray-100 flex items-start justify-between gap-4">
          <div>
            <h3 class="text-lg font-bold text-gray-900">
              {{ t("appList.pathDetails.title", { name: detailApp.name }) }}
            </h3>
            <p class="text-sm text-gray-500 mt-1">
              {{ t("appList.pathDetails.subtitle", { count: appPathCount(detailApp) }) }}
            </p>
          </div>
          <button
            type="button"
            data-test="app-path-details-close-top"
            @click="onClosePathDetails"
            class="px-3 py-1 text-sm text-gray-600 hover:bg-gray-100 rounded-lg"
          >
            {{ t("appList.pathDetails.close") }}
          </button>
        </div>
        <div class="p-6 bg-gray-50/50 flex-1 min-h-0 overflow-y-auto">
          <div v-if="detailHasTabs" class="mb-4 flex items-center gap-2 overflow-x-auto">
            <button
              v-for="group in detailGroups.filter((item) => item.key !== '__default__')"
              :key="group.key"
              :class="`px-3 py-1.5 rounded-lg text-sm whitespace-nowrap border ${
                activeDetailGroupKey === group.key
                  ? 'bg-blue-500 text-white border-blue-500'
                  : 'bg-white text-gray-700 border-gray-200 hover:border-blue-300'
              }`"
              @click="activeDetailGroupKey = group.key"
            >
              {{ group.label }}
            </button>
          </div>

          <div class="space-y-3">
            <div
              v-for="group in detailVisibleGroups"
              :key="`detail-${group.key}`"
              class="rounded-lg border border-gray-200 bg-white p-3"
            >
              <div
                v-if="!detailHasTabs && (detailVisibleGroups.length > 1 || group.key !== '__default__')"
                class="text-xs text-gray-500 mb-2"
              >
                {{ group.label }}
              </div>
              <div
                v-for="entry in groupPathEntries(group)"
                :key="entry.path"
                class="flex items-center gap-2 min-w-0 mb-1 last:mb-0"
              >
                <div class="min-w-0 max-w-lg">
                  <p
                    v-if="normalizedDisplayName(entry.displayName, group.key)"
                    class="text-xs text-gray-600 truncate"
                  >
                    {{ normalizedDisplayName(entry.displayName, group.key) }}
                  </p>
                </div>
                <span
                  v-if="entry.pending || entry.migrated"
                  class="text-[11px] px-1.5 py-0.5 rounded-full border whitespace-nowrap"
                  :class="entry.migrated
                    ? 'bg-green-100 text-green-700 border-green-200'
                    : 'bg-amber-100 text-amber-700 border-amber-200'"
                >
                  {{ entry.migrated ? t("appList.pathStatus.migrated") : t("appList.pathStatus.pending") }}
                </span>
                <button
                  type="button"
                  data-test="app-open-path-btn"
                  :disabled="props.loading || !hasActionablePath(entry.path)"
                  @click.stop.prevent="onOpenInFinder(entry.path)"
                  class="text-xs text-gray-600 hover:text-gray-800 whitespace-nowrap disabled:text-gray-400"
                >
                  {{ t("appList.pathActions.openInFinder") }}
                </button>
                <button
                  type="button"
                  data-test="app-copy-path-btn"
                  :disabled="props.loading || !hasActionablePath(entry.path)"
                  @click.stop.prevent="onCopyPath(detailApp.id, entry.path)"
                  class="text-xs text-blue-600 hover:text-blue-700 whitespace-nowrap disabled:text-gray-400"
                >
                  {{
                    isPathCopied(detailApp.id, entry.path)
                      ? t("appList.pathActions.copied")
                      : t("appList.pathActions.copyPath")
                  }}
                </button>
              </div>
            </div>
          </div>
        </div>
        <div class="p-4 border-t border-gray-100 bg-white flex justify-end">
          <button
            type="button"
            data-test="app-path-details-close-bottom"
            @click="onClosePathDetails"
            class="px-4 py-2 text-sm text-gray-700 hover:bg-gray-100 rounded-lg"
          >
            {{ t("appList.pathDetails.close") }}
          </button>
        </div>
      </div>
    </div>

    <div v-if="!props.loading && props.apps.length === 0" class="mt-6 rounded-xl border border-gray-200 bg-white px-4 py-6 text-sm text-gray-500">
      {{ t("appList.empty") }}
    </div>
  </div>
</template>
