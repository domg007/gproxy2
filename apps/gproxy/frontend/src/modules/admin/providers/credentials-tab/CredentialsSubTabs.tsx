import type { Dispatch, SetStateAction } from "react";

import type { CredentialsSubTab } from "../index";
import type { TranslateFn } from "./shared";

export function CredentialsSubTabs({
  subTab,
  setSubTab,
  t
}: {
  subTab: CredentialsSubTab;
  setSubTab: Dispatch<SetStateAction<CredentialsSubTab>>;
  t: TranslateFn;
}) {
  return (
    <div className="flex flex-wrap gap-2">
      <button
        type="button"
        className={`workspace-tab ${subTab === "single" ? "workspace-tab-active" : ""}`}
        onClick={() => setSubTab("single")}
      >
        {t("providers.subtab.single")}
      </button>
      <button
        type="button"
        className={`workspace-tab ${subTab === "bulk" ? "workspace-tab-active" : ""}`}
        onClick={() => setSubTab("bulk")}
      >
        {t("providers.subtab.bulk")}
      </button>
    </div>
  );
}
