import {invoke} from '@tauri-apps/api/core';import type {JourneySnapshot} from './types';
/** Canonical desktop read; no JSONL source scan or unsupported mutation fallback. */
export async function loadJourneySnapshot(ownerSessionId:string):Promise<JourneySnapshot>{
 const result=await invoke<JourneySnapshot>('journey_snapshot',{ownerSessionId});
 if(result.ownerSessionId!==ownerSessionId||result.schemaVersion!==1)throw Error('Journey canonical owner/schema mismatch');
 return result;
}

import type {DurableAnchor} from "./types";
export interface BoundaryPreview {ownerSessionId:string;anchor:DurableAnchor;prefixHash:string;records:Array<{durableMessageId:string;sequence:number;role:string;content:string}>;}
export async function loadJourneyBoundaryPreview(ownerSessionId:string,anchor:DurableAnchor):Promise<BoundaryPreview>{if(!ownerSessionId||!anchor||anchor.ownerSessionId!==ownerSessionId||(!Number.isSafeInteger(anchor.boundaryVersion)||anchor.boundaryVersion<0)||!anchor.boundaryHash||!anchor.sourceSessionId||!anchor.branchId||!anchor.durableMessageId)throw Error("Journey boundary request mismatch");const result=await invoke<BoundaryPreview>("journey_boundary_preview",{ownerSessionId,anchor});if(result.ownerSessionId!==ownerSessionId||!result.anchor||result.anchor.ownerSessionId!==anchor.ownerSessionId||result.anchor.sourceSessionId!==anchor.sourceSessionId||result.anchor.branchId!==anchor.branchId||result.anchor.durableMessageId!==anchor.durableMessageId||result.anchor.boundaryVersion!==anchor.boundaryVersion||result.anchor.boundaryHash!==anchor.boundaryHash||!result.prefixHash)throw Error("Journey boundary preview mismatch");return result;}

export interface JourneyDomainSnapshot {revision:number;snapshot:{session_id:string;active_branch_id:string;tasks:Record<string,{id:string;name:string;branch_id:string;state:string}>;branches:Record<string,{id:string;name:string;parent_branch_id:string;parent_anchor_message_id?:string;state:string}>;checkpoints:Record<string,{id:string;name:string;message_id:string}>;facts:Record<string,unknown>;reviews:Record<string,unknown>};}
export async function loadJourneyDomainSnapshot(ownerSessionId:string):Promise<JourneyDomainSnapshot>{
 if(!ownerSessionId||ownerSessionId.startsWith("cli:"))throw Error("Journey domain owner invalid");
 const result=await invoke<JourneyDomainSnapshot>("journey_domain_snapshot",{ownerSessionId});
 const record=(v:unknown):v is Record<string,unknown>=>typeof v==="object"&&v!==null&&!Array.isArray(v);
 if(!record(result)||!record(result.snapshot)||!Number.isSafeInteger(result.revision)||result.revision<0||typeof result.snapshot.active_branch_id!=="string"||![result.snapshot.tasks,result.snapshot.branches,result.snapshot.checkpoints,result.snapshot.facts,result.snapshot.reviews].every(record))throw Error("Journey domain response invalid");
 if(result.snapshot.session_id!==ownerSessionId)throw Error("Journey domain owner mismatch");return result;
}

/** Explicit user gesture only; native validates durable existence before rehydrating. */
export async function openDurableJourneyOwner(ownerSessionId:string):Promise<void>{
 if(!ownerSessionId||ownerSessionId.startsWith("cli:"))throw Error("Journey owner invalid");
 const snapshot=await invoke<JourneySnapshot>("journey_owner_open",{ownerSessionId});if(!snapshot||snapshot.schemaVersion!==1||!Number.isSafeInteger(snapshot.revision)||!Array.isArray(snapshot.nodes)||!Array.isArray(snapshot.edges))throw Error("Journey open snapshot invalid");if(snapshot.ownerSessionId!==ownerSessionId)throw Error("Journey owner mismatch");if(!snapshot.capabilities||snapshot.capabilities.read!==true)throw Error("Journey read capability unavailable");
}

export type ReviewJobState = "queued" | "running" | "ready" | "failed" | "cancelled";
export interface ReviewJobView {
  job_id: string;
  review_id: string;
  fork_id: string;
  session_id: string;
  state: ReviewJobState;
  attempt: number;
  error: string | null;
  model_id: string;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
}
const JOB_STATES: readonly string[] = ["queued", "running", "ready", "failed", "cancelled"];
function checkJob(job: ReviewJobView, ownerSessionId: string): ReviewJobView {
  if (!job || job.session_id !== ownerSessionId || !job.job_id || !JOB_STATES.includes(job.state)) throw Error("Journey review job mismatch");
  return job;
}
export async function loadJourneyReviewJobs(ownerSessionId: string): Promise<ReviewJobView[]> {
  const jobs = await invoke<ReviewJobView[]>("journey_review_jobs", { ownerSessionId });
  if (!Array.isArray(jobs)) throw Error("Journey review jobs invalid");
  return jobs.map((job) => checkJob(job, ownerSessionId));
}
export async function closeJourneyForkForReview(ownerSessionId: string, request: { expectedRevision: number; forkId: string; reviewId: string; messageId: string }): Promise<ReviewJobView> {
  const job = await invoke<ReviewJobView>("journey_fork_close", { ownerSessionId, request: { sessionId: ownerSessionId, outcome: "completed", ...request } });
  return checkJob(job, ownerSessionId);
}
export async function cancelJourneyReviewJob(ownerSessionId: string, jobId: string): Promise<ReviewJobView> {
  const job = await invoke<ReviewJobView>("journey_review_cancel", { ownerSessionId, request: { sessionId: ownerSessionId, jobId } });
  return checkJob(job, ownerSessionId);
}
