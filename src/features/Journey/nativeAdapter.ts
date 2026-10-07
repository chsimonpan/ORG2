import {invoke} from '@tauri-apps/api/core';import type {JourneySnapshot} from './types';
/** Canonical desktop read; no JSONL source scan or unsupported mutation fallback. */
export async function loadJourneySnapshot(ownerSessionId:string):Promise<JourneySnapshot>{
 const result=await invoke<JourneySnapshot>('journey_snapshot',{ownerSessionId});
 if(result.ownerSessionId!==ownerSessionId||result.schemaVersion!==1)throw Error('Journey canonical owner/schema mismatch');
 return result;
}

import type {DurableAnchor} from "./types";
export interface BoundaryPreview {ownerSessionId:string;anchor:DurableAnchor;prefixHash:string;records:Array<{durableMessageId:string;sequence:number;role:string;content:string}>;}
export async function loadJourneyBoundaryPreview(ownerSessionId:string,anchor:DurableAnchor):Promise<BoundaryPreview>{const result=await invoke<BoundaryPreview>("journey_boundary_preview",{ownerSessionId,anchor});if(result.ownerSessionId!==ownerSessionId||result.anchor.boundaryHash!==anchor.boundaryHash||!result.prefixHash)throw Error("Journey boundary preview mismatch");return result;}
