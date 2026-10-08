import {useEffect,useRef,useState} from "react";
import type {DurableAnchor} from "./types";
import {importSelectedJourneyContext,listImportedJourneySnapshots} from "./contextImportNative";
import ContextImportCard from "../../engines/ChatPanel/blocks/ToolCallBlock/cards/ContextImportCard";
import {parseContextImportCardResult} from "../../engines/ChatPanel/blocks/ToolCallBlock/helpers/cardParsers";
import type {ContextImportCardData} from "../../engines/ChatPanel/blocks/ToolCallBlock/types";
function requireCard(snapshot:Record<string,unknown>):ContextImportCardData {const card=parseContextImportCardResult({},snapshot);if(!card)throw Error("Native context import card metadata mismatch");return card;}
/** User-selected import only. Rendering never imports, calls a model, or chooses another session. */
export function NativeContextImportControls({ownerSessionId,anchor}:{ownerSessionId:string;anchor:DurableAnchor|null}){
 const [pending,setPending]=useState(false),[error,setError]=useState<string|null>(null),[cards,setCards]=useState<ContextImportCardData[]>([]);
 const cardOwner=useRef(ownerSessionId);
 const scope=useRef({owner:ownerSessionId,generation:0,request:0});
 if(scope.current.owner!==ownerSessionId){scope.current={owner:ownerSessionId,generation:scope.current.generation+1,request:scope.current.request+1};}
 useEffect(()=>()=>{scope.current.generation++;scope.current.request++;},[]);
 useEffect(()=>{
  let live=true;const generation=scope.current.generation,request=scope.current.request;setPending(false);setError(null);setCards([]);cardOwner.current=ownerSessionId;
  void listImportedJourneySnapshots(ownerSessionId).then(snapshots=>{if(live&&scope.current.owner===ownerSessionId&&scope.current.generation===generation&&scope.current.request===request)setCards(snapshots.map(snapshot=>requireCard(snapshot as unknown as Record<string,unknown>)));}).catch(e=>{if(live&&scope.current.owner===ownerSessionId&&scope.current.generation===generation&&scope.current.request===request)setError(String(e));});
  return ()=>{live=false;};
 },[ownerSessionId]);
 const run=async()=>{
  if(pending||!anchor)return;
  const generation=scope.current.generation,request=++scope.current.request;
  const current=()=>scope.current.owner===ownerSessionId&&scope.current.generation===generation&&scope.current.request===request;
  setPending(true);setError(null);
  try{const snapshot=await importSelectedJourneyContext(ownerSessionId,anchor);if(current())setCards(previous=>[...previous,requireCard(snapshot as unknown as Record<string,unknown>)]);}
  catch(e){if(current())setError(String(e));}finally{if(current())setPending(false);}
 };
 return <section aria-label="Native context import"><button type="button" disabled={pending||!anchor} onClick={()=>void run()}>Import selected context</button>{error&&<p role="alert">{error}</p>}{cardOwner.current===ownerSessionId&&cards.map((card,index)=><ContextImportCard key={card.snapshotId??index} card={card}/>)}</section>;
}
