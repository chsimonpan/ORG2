import {invoke} from '@tauri-apps/api/core';import type {JourneySnapshot} from './types';
/** Canonical desktop read; no JSONL source scan or unsupported mutation fallback. */
export async function loadJourneySnapshot(ownerSessionId:string):Promise<JourneySnapshot>{
 const result=await invoke<JourneySnapshot>('journey_snapshot',{ownerSessionId});
 if(result.ownerSessionId!==ownerSessionId||result.schemaVersion!==1)throw Error('Journey canonical owner/schema mismatch');
 return result;
}
