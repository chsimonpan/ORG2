import {useEffect,useRef,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import type {DurableAnchor} from "./types";
/** A physical runtime child, not a named branch inside the existing owner. */
export function JourneyHistoricalForkControls({ownerSessionId,anchor,onCommitted}:{ownerSessionId:string;anchor:DurableAnchor|null;onCommitted:(child:string)=>void}){
 const [busy,setBusy]=useState(false),[error,setError]=useState<string|null>(null);
 const authority=useRef({owner:ownerSessionId,epoch:0,busy:false});
 const intent=useRef<{key:string;id:string}|null>(null);
 if(authority.current.owner!==ownerSessionId){authority.current={owner:ownerSessionId,epoch:authority.current.epoch+1,busy:false};intent.current=null;setBusy(false);setError(null);}
 useEffect(()=>()=>{authority.current.epoch++;authority.current.busy=false;},[]);
 const run=async()=>{
  if(authority.current.busy||!anchor||anchor.ownerSessionId!==ownerSessionId||anchor.sourceSessionId!==ownerSessionId)return;
  const key=JSON.stringify(anchor);if(intent.current?.key!==key)intent.current={key,id:crypto.randomUUID()};
  const request={anchor:{...anchor},intentId:intent.current.id};const epoch=authority.current.epoch;
  const current=()=>authority.current.owner===ownerSessionId&&authority.current.epoch===epoch;
  authority.current.busy=true;setBusy(true);setError(null);
  try{const result=await invoke<{childSessionId:string;parentSessionId:string}>("historical_runtime_fork",{ownerSessionId,request});
   if(current()){if(result.parentSessionId!==ownerSessionId||!result.childSessionId||result.childSessionId===ownerSessionId)throw new Error("historical_fork_invalid_response");onCommitted(result.childSessionId);intent.current=null;}
  }catch(e){if(current())setError(String(e));}finally{if(current()){authority.current.busy=false;setBusy(false);}}
 };
 return <section aria-label="Historical runtime fork"><button disabled={busy||!anchor||anchor.ownerSessionId!==ownerSessionId||anchor.sourceSessionId!==ownerSessionId} onClick={()=>void run()}>Continue in new runtime from exact endpoint</button>{error&&<p role="alert">{error} — retry the same endpoint to recover this operation.</p>}</section>;
}
