import {invoke} from "@tauri-apps/api/core";
import type {DurableAnchor} from "./types";
export interface NativeContextImportSnapshot {snapshotId:string;targetSessionId:string;sourceKind:"session";sourceId:string;namespace:string;title:string;tokenEstimate:number;pinned:boolean;snippet:string;anchor:DurableAnchor;prefixHash:string;}
export async function importSelectedJourneyContext(ownerSessionId:string,anchor:DurableAnchor,tokenBudget=4096):Promise<NativeContextImportSnapshot>{
 if(!ownerSessionId||ownerSessionId.startsWith("cli:")||anchor.ownerSessionId!==ownerSessionId||anchor.sourceSessionId!==ownerSessionId||anchor.branchId!==`session:${ownerSessionId}`||!anchor.durableMessageId||!anchor.boundaryHash||!Number.isSafeInteger(anchor.boundaryVersion)||anchor.boundaryVersion<0||!Number.isSafeInteger(tokenBudget)||tokenBudget<1||tokenBudget>32768)throw Error("Native context import authority mismatch");
 const result=await invoke<NativeContextImportSnapshot>("context_import_selected",{ownerSessionId,anchor,tokenBudget,pinned:true});
 if(!result||result.targetSessionId!==ownerSessionId||result.sourceKind!=="session"||result.sourceId!==ownerSessionId||result.namespace!==`session:${ownerSessionId}`||!result.snapshotId||!result.prefixHash||!result.anchor||Object.keys(anchor).some(key=>result.anchor[key as keyof DurableAnchor]!==anchor[key as keyof DurableAnchor])||!Number.isSafeInteger(result.tokenEstimate)||result.tokenEstimate<0||result.tokenEstimate>tokenBudget)throw Error("Native context import response mismatch");
 return result;
}

/** Read-only card hydration: no automatic imports and no provider calls. */
export async function listImportedJourneySnapshots(ownerSessionId:string):Promise<NativeContextImportSnapshot[]>{
 if(!ownerSessionId||ownerSessionId.startsWith("cli:"))throw Error("Native context import authority mismatch");
 const snapshots=await invoke<NativeContextImportSnapshot[]>("context_import_snapshots",{ownerSessionId});
 if(!Array.isArray(snapshots)||snapshots.length>10000)throw Error("Native context import list mismatch");
 const ids=new Set<string>();let budget=0;
 for(const s of snapshots){
  if(!s||s.targetSessionId!==ownerSessionId||s.sourceKind!=="session"||s.sourceId!==ownerSessionId||s.namespace!==`session:${ownerSessionId}`||!s.snapshotId||ids.has(s.snapshotId)||!s.prefixHash||typeof s.snippet!=="string"||typeof s.title!=="string"||typeof s.pinned!=="boolean"||!Number.isSafeInteger(s.tokenEstimate)||s.tokenEstimate<0||s.tokenEstimate>32768||!s.anchor||s.anchor.ownerSessionId!==ownerSessionId||s.anchor.sourceSessionId!==ownerSessionId||s.anchor.branchId!==`session:${ownerSessionId}`||!s.anchor.durableMessageId||!s.anchor.boundaryHash||!Number.isSafeInteger(s.anchor.boundaryVersion)||s.anchor.boundaryVersion<0)throw Error("Native context import list mismatch");
  ids.add(s.snapshotId);budget+=s.tokenEstimate;if(budget>32768)throw Error("Native context import list budget exceeded");
 }
 return snapshots;
}
