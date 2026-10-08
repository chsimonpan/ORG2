import {invoke} from "@tauri-apps/api/core";
import type {DurableAnchor} from "./types";
export interface NativeContextImportSnapshot {snapshotId:string;targetSessionId:string;sourceKind:"session";sourceId:string;namespace:string;title:string;tokenEstimate:number;pinned:boolean;snippet:string;anchor:DurableAnchor;prefixHash:string;}
export async function importSelectedJourneyContext(ownerSessionId:string,anchor:DurableAnchor,tokenBudget=4096):Promise<NativeContextImportSnapshot>{
 if(!ownerSessionId||ownerSessionId.startsWith("cli:")||anchor.ownerSessionId!==ownerSessionId||anchor.sourceSessionId!==ownerSessionId||anchor.branchId!==`session:${ownerSessionId}`||!anchor.durableMessageId||!anchor.boundaryHash||!Number.isSafeInteger(anchor.boundaryVersion)||anchor.boundaryVersion<0||!Number.isSafeInteger(tokenBudget)||tokenBudget<1||tokenBudget>32768)throw Error("Native context import authority mismatch");
 const result=await invoke<NativeContextImportSnapshot>("context_import_selected",{ownerSessionId,anchor,tokenBudget,pinned:true});
 if(!result||result.targetSessionId!==ownerSessionId||result.sourceKind!=="session"||result.sourceId!==ownerSessionId||result.namespace!==`session:${ownerSessionId}`||!result.snapshotId||!result.prefixHash||!result.anchor||Object.keys(anchor).some(key=>result.anchor[key as keyof DurableAnchor]!==anchor[key as keyof DurableAnchor])||!Number.isSafeInteger(result.tokenEstimate)||result.tokenEstimate<0||result.tokenEstimate>tokenBudget)throw Error("Native context import response mismatch");
 return result;
}
