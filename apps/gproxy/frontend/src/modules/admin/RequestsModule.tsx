import { Card } from "../../components/ui";
import { useI18n } from "../../app/i18n";
import { RequestFilters } from "./requests/RequestFilters";
import { RequestsTable } from "./requests/RequestsTable";
import { useRequestsModuleState } from "./requests/useRequestsModuleState";

export function RequestsModule({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
}) {
  const { t } = useI18n();
  const {
    kind,
    setKind,
    filters,
    updateFilter,
    rows,
    pageSize,
    setPageSize,
    page,
    setPage,
    totalRows,
    totalPages,
    canGoNext,
    loadingRows,
    loadingCount,
    clearingPayload,
    selectedTraceIds,
    bodyByTraceId,
    bodyLoadingByTraceId,
    bodyErrorByTraceId,
    isFilterOptionsLoading,
    providerOptions,
    filteredCredentialOptions,
    userOptions,
    filteredUserKeyOptions,
    requestPathOptions,
    runQuery,
    ensureBodyLoaded,
    toggleTraceIdSelected,
    clearPayload
  } = useRequestsModuleState({
    apiKey,
    notify,
    t
  });

  return (
    <Card title={t("requests.title")} subtitle={t("requests.subtitle")}>
      <div className="request-body-hint mb-3 text-xs">{t("requests.bodyHint")}</div>
      <RequestFilters
        kind={kind}
        onKindChange={setKind}
        filters={filters}
        onFilterChange={updateFilter}
        providerOptions={providerOptions}
        credentialOptions={filteredCredentialOptions}
        userOptions={userOptions}
        userKeyOptions={filteredUserKeyOptions}
        requestPathOptions={requestPathOptions}
        isFilterOptionsLoading={isFilterOptionsLoading}
        loadingRows={loadingRows}
        loadingCount={loadingCount}
        clearingPayload={clearingPayload}
        selectedCount={selectedTraceIds.length}
        onRunQuery={runQuery}
        onClearPayload={(all) => void clearPayload(all)}
        t={t}
      />
      <RequestsTable
        kind={kind}
        rows={rows}
        bodyByTraceId={bodyByTraceId}
        bodyLoadingByTraceId={bodyLoadingByTraceId}
        bodyErrorByTraceId={bodyErrorByTraceId}
        ensureBodyLoaded={ensureBodyLoaded}
        selectedTraceIds={selectedTraceIds}
        clearingPayload={clearingPayload}
        onToggleTraceIdSelected={toggleTraceIdSelected}
        totalRows={totalRows}
        pageSize={pageSize}
        onPageSizeChange={setPageSize}
        page={page}
        totalPages={totalPages}
        canGoNext={canGoNext}
        loadingRows={loadingRows}
        loadingCount={loadingCount}
        onPageChange={setPage}
        notify={notify}
        t={t}
      />
    </Card>
  );
}
