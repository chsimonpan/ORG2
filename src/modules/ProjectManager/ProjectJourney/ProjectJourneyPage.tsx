import React from "react";

import type { JourneyScope } from "@src/api/tauri/journeyGraph";
import { JourneyContainer } from "@src/modules/ProjectManager/JourneyGraph";

export interface ProjectJourneyPageProps {
  projectId?: string;
  projectSlug?: string;
  projectName?: string;
  forceDemo?: boolean;
}

/** Read-only project wrapper over the shared Journey graph container.
 *
 * A Journey scope is a canonical project id, never a display slug. Slugs can
 * be renamed and are only retained for tab chrome/backward compatibility.
 */
const ProjectJourneyPage: React.FC<ProjectJourneyPageProps> = ({
  projectId,
  projectName,
}) => {
  // Journey ownership requires an explicit canonical project id. Slugs are
  // labels only and must never be promoted into ownership by inference.
  const identity = projectId;
  if (!identity)
    return (
      <div className="p-3 text-xs text-warning-6" role="alert">
        项目旅程不可用：未绑定 journey project_id，拒绝推断项目归属。
      </div>
    );
  return (
    <JourneyContainer
      scope={`project/${identity}` as JourneyScope}
      title={`项目旅程${projectName ? ` · ${projectName}` : ""} · journey project_id: ${identity}`}
    />
  );
};
export default ProjectJourneyPage;
