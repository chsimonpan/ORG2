import React, { Suspense, lazy, memo } from "react";

import type { useSessionRawTranscript } from "@src/engines/ChatPanel/components/SessionRawTranscriptDialog/useSessionRawTranscript";

// Lazy: the transcript content pulls CodeMirror, and this view only mounts
// when the user switches a session to the Raw alternate view.
const SessionRawTranscriptContent = lazy(
  () =>
    import("@src/engines/ChatPanel/components/SessionRawTranscriptDialog/SessionRawTranscriptContent")
);

export interface SessionRawTranscriptViewProps {
  sessionId: string;
  transcript: ReturnType<typeof useSessionRawTranscript>;
}

const SessionRawTranscriptView: React.FC<SessionRawTranscriptViewProps> = memo(
  ({ sessionId, transcript }) => {
    return (
      <div
        data-testid="workstation-session-raw-view"
        className="flex min-h-0 flex-1 flex-col"
      >
        <Suspense fallback={null}>
          <SessionRawTranscriptContent
            error={transcript.error}
            filePath={`raw-transcript-${sessionId}.json`}
            loaded={Boolean(transcript.snapshot)}
            loading={transcript.loading}
            transcriptJson={transcript.transcriptJson}
          />
        </Suspense>
      </div>
    );
  }
);

SessionRawTranscriptView.displayName = "SessionRawTranscriptView";

export default SessionRawTranscriptView;
