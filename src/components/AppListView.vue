<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onBeforeUnmount, reactive, ref } from "vue";
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
  desc: string;
  availability: "active" | "blocked" | "deprecated";
  blockedReason?: string | null;
  requiresConfirmation?: boolean;
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
const copiedPathAppId = ref<string | null>(null);

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
  if (app.requiresConfirmation) {
    return t("appList.hint.requiresConfirmation");
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

function hasActionablePath(rawPath: string): boolean {
  return normalizePath(rawPath).startsWith("/");
}

async function onCopyPath(app: AppCard): Promise<void> {
  const path = normalizePath(app.path);
  if (!hasActionablePath(path)) {
    return;
  }

  try {
    if (!navigator.clipboard?.writeText) {
      throw new Error("clipboard API unavailable");
    }
    await navigator.clipboard.writeText(path);
    copiedPathAppId.value = app.id;
    pathActionError.value = null;
    stopCopyFeedbackTimer();
    copyFeedbackTimer = window.setTimeout(() => {
      copiedPathAppId.value = null;
      copyFeedbackTimer = null;
    }, 1200);
  } catch (err) {
    pathActionError.value = t("appList.pathActions.copyFailed", {
      error: formatCommandError(err)
    });
  }
}

async function onOpenInFinder(app: AppCard): Promise<void> {
  const path = normalizePath(app.path);
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
            <span
              v-else-if="app.requiresConfirmation"
              class="bg-yellow-100 text-yellow-700 text-xs px-2 py-0.5 rounded-full font-medium"
            >
              {{ t("appList.status.requiresConfirmation") }}
            </span>
          </div>
          <p class="text-sm text-gray-500 mt-1">{{ app.desc }}</p>
          <p class="text-xs text-gray-400 mt-1">{{ migrationHint(app) }}</p>
          <div class="mt-1 flex items-center gap-2 min-w-0">
            <p class="text-xs text-gray-400 font-mono truncate max-w-md" :title="app.path">{{ app.path }}</p>
            <button
              type="button"
              data-test="app-open-path-btn"
              :disabled="props.loading || !hasActionablePath(app.path)"
              @click.stop.prevent="onOpenInFinder(app)"
              class="text-xs text-gray-600 hover:text-gray-800 whitespace-nowrap disabled:text-gray-400"
            >
              {{ t("appList.pathActions.openInFinder") }}
            </button>
            <button
              type="button"
              data-test="app-copy-path-btn"
              :disabled="props.loading || !hasActionablePath(app.path)"
              @click.stop.prevent="onCopyPath(app)"
              class="text-xs text-blue-600 hover:text-blue-700 whitespace-nowrap disabled:text-gray-400"
            >
              {{
                copiedPathAppId === app.id
                  ? t("appList.pathActions.copied")
                  : t("appList.pathActions.copyPath")
              }}
            </button>
          </div>
        </div>

        <div class="text-right px-4">
          <div class="text-lg font-bold text-gray-700">{{ app.size }}</div>
          <div class="text-xs text-gray-400">{{ app.sizeLabel || t("appList.sizeLabelCurrent") }}</div>
        </div>

        <div class="flex-shrink-0 border-l border-gray-100 pl-5">
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
            v-else
            type="button"
            @click="emit('restore', app.id)"
            class="bg-gray-100 hover:bg-gray-200 text-gray-700 px-5 py-2 rounded-lg font-medium transition-colors"
          >
            {{ t("appList.restore") }}
          </button>
        </div>
      </div>
    </div>

    <div v-if="!props.loading && props.apps.length === 0" class="mt-6 rounded-xl border border-gray-200 bg-white px-4 py-6 text-sm text-gray-500">
      {{ t("appList.empty") }}
    </div>
  </div>
</template>
