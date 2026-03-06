<script setup lang="ts">
import { reactive } from "vue";
import { useI18n } from "../i18n";

interface AppCard {
  id: string;
  name: string;
  icon: string;
  iconPath: string | null;
  size: string;
  isMigrated: boolean;
  targetDisk: string | null;
  path: string;
  desc: string;
  tier: "supported" | "experimental" | "blocked";
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

function migrationHint(app: AppCard): string {
  if (app.tier === "blocked") {
    return t("appList.hint.blocked");
  }
  if (app.running) {
    return t("appList.hint.running");
  }
  if (app.tier === "experimental") {
    return t("appList.hint.experimental");
  }
  if (app.isMigrated) {
    return t("appList.hint.migrated");
  }
  return t("appList.hint.ready");
}

function canMigrate(app: AppCard): boolean {
  return app.tier !== "blocked" && !app.running && !app.isMigrated;
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
            <span v-if="app.tier === 'experimental'" class="bg-yellow-100 text-yellow-700 text-xs px-2 py-0.5 rounded-full font-medium">
              {{ t("appList.tier.experimental") }}
            </span>
            <span v-if="app.tier === 'blocked'" class="bg-red-100 text-red-700 text-xs px-2 py-0.5 rounded-full font-medium">
              {{ t("appList.tier.blocked") }}
            </span>
          </div>
          <p class="text-sm text-gray-500 mt-1">{{ app.desc }}</p>
          <p class="text-xs text-gray-400 mt-1">{{ migrationHint(app) }}</p>
          <p class="text-xs text-gray-400 mt-1 font-mono truncate max-w-md" :title="app.path">{{ app.path }}</p>
        </div>

        <div class="text-right px-4">
          <div class="text-lg font-bold text-gray-700">{{ app.size }}</div>
          <div class="text-xs text-gray-400">{{ t("appList.sizeLabel") }}</div>
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
