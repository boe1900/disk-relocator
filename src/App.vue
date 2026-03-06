<script setup lang="ts">
import AppListView from "./components/AppListView.vue";
import HealthPanelView from "./components/HealthPanelView.vue";
import MigrationDialog from "./components/MigrationDialog.vue";
import MigrationStatusView from "./components/MigrationStatusView.vue";
import OperationLogExportView from "./components/OperationLogExportView.vue";
import ReconcileTaskView from "./components/ReconcileTaskView.vue";
</script>

<template>
  <main>
    <h1>Disk Relocator (Scaffold)</h1>
    <p class="muted">当前版本包含迁移闭环、健康恢复与对账任务调试面板。</p>
    <section class="panel-grid">
      <AppListView />
      <MigrationDialog />
      <MigrationStatusView />
      <HealthPanelView />
      <OperationLogExportView />
      <ReconcileTaskView />
    </section>
  </main>
</template>
