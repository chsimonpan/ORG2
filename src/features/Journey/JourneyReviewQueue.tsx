import { useCallback, useEffect, useRef, useState } from "react";

import Button from "@src/components/Button";

import {
  cancelJourneyReviewJob,
  closeJourneyForkForReview,
  loadJourneyReviewJobs,
} from "./nativeAdapter";
import type { JourneyDomainSnapshot, ReviewJobView } from "./nativeAdapter";
import type { DurableAnchor } from "./types";

const POLL_MS = 3000;
const isLive = (job: ReviewJobView) =>
  job.state === "queued" || job.state === "running";

/** Review queue for the active fork: close at an exact endpoint, watch the job, cancel it. Rendering never calls a model. */
export function JourneyReviewQueue({
  ownerSessionId,
  domain,
  anchor,
  onChanged,
}: {
  ownerSessionId: string;
  domain: JourneyDomainSnapshot;
  anchor: DurableAnchor | null;
  onChanged: () => void;
}) {
  const [jobs, setJobs] = useState<ReviewJobView[]>([]);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const lastStates = useRef<string>("");
  const changed = useRef(onChanged);
  changed.current = onChanged;

  const pollFailed = useRef(false);
  const refresh = useCallback(async () => {
    const mine = generation.current;
    try {
      const next = await loadJourneyReviewJobs(ownerSessionId);
      if (generation.current !== mine) return;
      setJobs(next);
      if (pollFailed.current) {
        pollFailed.current = false;
        setError(null);
      }
      const states = next.map((job) => `${job.job_id}:${job.state}`).join("|");
      if (lastStates.current && lastStates.current !== states)
        changed.current();
      lastStates.current = states;
    } catch (e) {
      if (generation.current === mine) {
        pollFailed.current = true;
        setError(String(e));
      }
    }
  }, [ownerSessionId]);

  useEffect(() => {
    generation.current++;
    lastStates.current = "";
    setJobs([]);
    setError(null);
    return () => {
      // generation is a plain counter, not a DOM ref: bumping it invalidates in-flight refreshes
      // eslint-disable-next-line react-hooks/exhaustive-deps
      generation.current++;
    };
  }, [ownerSessionId]);

  useEffect(() => {
    void refresh();
  }, [refresh, domain.revision]);

  const polling = jobs.some(isLive);
  useEffect(() => {
    if (!polling) return;
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void refresh();
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [polling, refresh]);

  const forkId = domain.snapshot.active_branch_id;
  const fork = domain.snapshot.branches[forkId];
  const forkOpen =
    !!fork && fork.state === "active" && !!fork.parent_anchor_message_id;
  const canClose =
    forkOpen &&
    !pending &&
    !!anchor &&
    anchor.ownerSessionId === ownerSessionId &&
    anchor.sourceSessionId === ownerSessionId &&
    domain.snapshot.session_id === ownerSessionId;

  const run = async (action: () => Promise<unknown>) => {
    if (pending) return;
    const mine = generation.current;
    setPending(true);
    setError(null);
    try {
      await action();
      if (generation.current === mine) {
        await refresh();
        changed.current();
      }
    } catch (e) {
      if (generation.current === mine) setError(String(e));
    } finally {
      if (generation.current === mine) setPending(false);
    }
  };

  return (
    <section aria-label="Journey review queue">
      <strong>Fork review queue</strong>
      <div>
        <Button
          size="small"
          disabled={!canClose}
          onClick={() =>
            void run(() =>
              closeJourneyForkForReview(ownerSessionId, {
                expectedRevision: domain.revision,
                forkId,
                reviewId: crypto.randomUUID(),
                messageId: anchor!.durableMessageId,
              })
            )
          }
        >
          Close fork for review at selected endpoint
        </Button>
      </div>
      {jobs.length === 0 && <p>No review jobs</p>}
      <ul>
        {jobs.map((job) => (
          <li key={job.job_id} data-state={job.state}>
            <span>Review {job.review_id}</span>
            <span>
              {" "}
              {job.state} / attempt {job.attempt} / model {job.model_id}
            </span>
            {job.error && <p role="note">{job.error}</p>}
            {isLive(job) && (
              <Button
                size="small"
                disabled={pending}
                onClick={() =>
                  void run(() =>
                    cancelJourneyReviewJob(ownerSessionId, job.job_id)
                  )
                }
              >
                Cancel review {job.review_id}
              </Button>
            )}
          </li>
        ))}
      </ul>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
