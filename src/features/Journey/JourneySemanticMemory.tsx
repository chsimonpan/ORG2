import {useEffect,useRef,useState} from "react";
import {searchOwnerMemory,type MemorySearchHit} from "@src/api/tauri/memorySearch";
/** Semantic records are not transcript events and have no chat navigation authority. */
export function JourneySemanticMemory({ownerSessionId}:{ownerSessionId:string}){
 const scope=useRef({owner:ownerSessionId,generation:0,request:0,live:true});
 if(scope.current.owner!==ownerSessionId)scope.current={owner:ownerSessionId,generation:scope.current.generation+1,request:scope.current.request+1,live:true};
 const [query,setQuery]=useState("");
 const [state,setState]=useState<{generation:number;busy:boolean;hits:MemorySearchHit[]|null;error:string|null}>({generation:0,busy:false,hits:null,error:null});
 useEffect(()=>{scope.current.live=true;setQuery("");setState({generation:scope.current.generation,busy:false,hits:null,error:null});return()=>{scope.current.live=false;scope.current.request++;};},[ownerSessionId]);
 const visible=state.generation===scope.current.generation?state:{busy:false,hits:null,error:null};
 const invalidateQuery=(value:string)=>{scope.current.request++;setQuery(value);setState({generation:scope.current.generation,busy:false,hits:null,error:null});};
 const search=async()=>{const text=query.trim();if(!text)return;const s=scope.current,generation=s.generation,request=++s.request;const current=()=>scope.current.live&&scope.current.owner===ownerSessionId&&scope.current.generation===generation&&scope.current.request===request;setState({generation,busy:true,hits:null,error:null});try{const hits=await searchOwnerMemory(ownerSessionId,text,10);if(current())setState({generation,busy:false,hits,error:null});}catch(error){if(current())setState({generation,busy:false,hits:null,error:String(error)});}};
 return <section className="my-3 rounded border border-border-2 p-3" aria-label="语义记忆搜索区域"><h3>语义记忆</h3><p>搜索当前会话的语义记忆；结果不是聊天事件，不提供聊天定位。</p><form aria-label="语义记忆" onSubmit={e=>{e.preventDefault();void search();}}><label>查询<input aria-label="语义记忆查询" value={query} onChange={e=>invalidateQuery(e.target.value)} /></label><button type="submit" disabled={!query.trim()}>搜索语义记忆</button></form>{visible.busy&&<p role="status">搜索中…</p>}{visible.error&&<p role="alert">{visible.error}</p>}{visible.hits?.length===0&&<p role="status">没有匹配的语义记忆</p>}<ul>{visible.hits?.map((hit,index)=><li key={`${hit.sourceId}:${index}`}><p className="whitespace-pre-wrap">{hit.content}</p><small>sourceId: {hit.sourceId} · score: {hit.score}</small></li>)}</ul></section>;
}
