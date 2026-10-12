// @vitest-environment jsdom
import {it,expect,vi} from "vitest";
import {createElement} from "react";
import {invoke} from "@tauri-apps/api/core";
import {createSmokeRoot,dispatch} from "@src/test/reactSmokeHarness";
import {JourneyOwnerPanel,JourneyPopover} from "./JourneyPopover";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn(()=>new Promise(()=>{}))}));
it("production mounted semantic entry fences A-B-A, superseded queries, errors and unmount via invoke only",async()=>{
 const root=createSmokeRoot();const pending:{resolve:(v:unknown)=>void;reject:(e:Error)=>void}[]=[];
 vi.mocked(invoke).mockImplementation((command)=>command==="memory_search"?new Promise((resolve,reject)=>pending.push({resolve,reject})):new Promise(()=>{}));
 const render=(ownerSessionId:string)=>root.render(createElement(JourneyOwnerPanel,{ownerSessionId,onClose:vi.fn()}));
 const search=async(q:string)=>{const input=root.container.querySelector<HTMLInputElement>('[aria-label="语义记忆查询"]')!;expect(input).not.toBeNull();await dispatch(()=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!.call(input,q);input.dispatchEvent(new Event("input",{bubbles:true}));});await dispatch(()=>root.container.querySelector('form[aria-label="语义记忆"]')!.dispatchEvent(new Event("submit",{bubbles:true,cancelable:true})));};
 const hit=(content:string)=>[{content,sourceId:"fixture-source",score:0.75}];
 try{await render("A");expect(root.container.textContent).toContain("语义记忆");await search("first");expect(invoke).toHaveBeenCalledWith("memory_search",{ownerSessionId:"A",query:"first",topN:10});expect(root.container.textContent).toContain("搜索中");await render("B");await search("second");await render("A");await search("third");await dispatch(()=>pending[0].resolve(hit("STALE-A")));await dispatch(()=>pending[1].reject(new Error("STALE-B")));expect(root.container.textContent).not.toContain("STALE");await dispatch(()=>pending[2].resolve(hit("CURRENT")));expect(root.container.textContent).toContain("CURRENT");expect(root.container.textContent).toContain("fixture-source");expect(root.container.textContent).toContain("0.75");await search("old");await search("new");await dispatch(()=>pending[3].resolve(hit("OBSOLETE")));expect(root.container.textContent).not.toContain("OBSOLETE");await dispatch(()=>pending[4].resolve([]));expect(root.container.textContent).toContain("没有匹配的语义记忆");await search("failure");await dispatch(()=>pending[5].reject(new Error("provider 503")));expect(root.container.textContent).toContain("provider 503");await search("unmount");await root.unmount();await dispatch(()=>pending[6].resolve(hit("UNMOUNTED")));}finally{await root.unmount();}
});
it("production JourneyPopover opens the semantic region with its owner",async()=>{vi.mocked(invoke).mockImplementation((command)=>command==="journey_owner_open"?Promise.resolve({schemaVersion:1,revision:1,ownerSessionId:"header-owner",nodes:[],edges:[],capabilities:{read:true}}):new Promise(()=>{}));const root=createSmokeRoot();try{await root.render(createElement(JourneyPopover,{ownerSessionId:"header-owner"}));await dispatch(()=>root.container.querySelector<HTMLButtonElement>('button[aria-label="Session Journey"]')!.click());expect(root.container.textContent).toContain("语义记忆");}finally{await root.unmount();}});
it("semantic hit source location loads only the exact owner prefix, marks the evidence message, fences stale locates and shows unavailable reasons",async()=>{
 const root=createSmokeRoot();const anchorA={ownerSessionId:"A",sourceSessionId:"A",branchId:"session:A",durableMessageId:"m2",durableTurnId:null,boundaryVersion:3,boundaryHash:"hash-m2"};
 const preview=(id:string)=>({ownerSessionId:"A",anchor:{...anchorA,durableMessageId:id},prefixHash:"prefix-"+id,records:[{durableMessageId:"m1",sequence:1,role:"user",content:"PREFIX-ONE"},{durableMessageId:"m2",sequence:2,role:"assistant",content:"EVIDENCE-TWO"}]});
 const hits=[{sourceId:"fact:f1",content:"fact one",score:0.9,sourceKind:"confirmed_fact",forkId:"fk",reviewId:"rv",sourceAnchor:anchorA,sourceUnavailable:null},{sourceId:"review:r9",content:"orphan",score:0.5,sourceKind:"review_annotation",forkId:"fk",reviewId:"r9",sourceAnchor:null,sourceUnavailable:"source_review_missing"}];
 let pendingPreview:{resolve:(v:unknown)=>void}[]=[];
 vi.mocked(invoke).mockImplementation((command)=>command==="memory_search"?Promise.resolve(hits):command==="journey_boundary_preview"?new Promise(resolve=>pendingPreview.push({resolve})):new Promise(()=>{}));
 const render=(ownerSessionId:string)=>root.render(createElement(JourneyOwnerPanel,{ownerSessionId,onClose:vi.fn()}));
 const search=async(q:string)=>{const input=root.container.querySelector<HTMLInputElement>('[aria-label="语义记忆查询"]')!;await dispatch(()=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!.call(input,q);input.dispatchEvent(new Event("input",{bubbles:true}));});await dispatch(()=>root.container.querySelector('form[aria-label="语义记忆"]')!.dispatchEvent(new Event("submit",{bubbles:true,cancelable:true})));};
 const locateButton=()=>root.container.querySelector<HTMLButtonElement>('button[aria-label="定位来源 fact:f1"]');
 try{
  await render("A");await search("q");
  expect(root.container.textContent).toContain("来源不可定位：source_review_missing");
  expect(root.container.querySelector('button[aria-label="定位来源 review:r9"]')).toBeNull();
  await dispatch(()=>locateButton()!.click());
  expect(invoke).toHaveBeenCalledWith("journey_boundary_preview",{ownerSessionId:"A",anchor:anchorA});
  expect(root.container.textContent).toContain("定位来源中");
  await dispatch(()=>pendingPreview[0].resolve(preview("m2")));
  const region=root.container.querySelector('[aria-label="语义记忆来源定位"]')!;expect(region).not.toBeNull();expect(region.textContent).toContain("EVIDENCE-TWO");expect(region.textContent).toContain("prefix-m2");
  expect(region.querySelectorAll('[data-source-evidence="true"]').length).toBe(1);expect(region.querySelector('[data-source-evidence="true"]')!.textContent).toContain("EVIDENCE-TWO");
  // A new query or an owner switch discards a located source; a late preview for the old request is dropped.
  pendingPreview=[];await dispatch(()=>locateButton()!.click());await render("B");await dispatch(()=>pendingPreview[0].resolve(preview("m2")));
  expect(root.container.querySelector('[aria-label="语义记忆来源定位"]')).toBeNull();expect(root.container.textContent).not.toContain("EVIDENCE-TWO");
 }finally{await root.unmount();}
});
it("embedding index status makes pending, retrying and retry-exhausted failures visible, and never shows another owner's status",async()=>{
 const root=createSmokeRoot();
 const status=(over:Record<string,unknown>)=>({queued:0,running:0,completed:0,failed:0,terminalFailed:0,indexed:0,maxAttempts:3,failures:[],...over});
 const byOwner:Record<string,unknown>={A:status({indexed:2})};let pendingB:{resolve:(v:unknown)=>void}[]=[];
 vi.mocked(invoke).mockImplementation((command,args)=>{const owner=(args as {ownerSessionId?:string}|undefined)?.ownerSessionId??"";
  if(command==="memory_index_status")return owner==="B"?new Promise(resolve=>pendingB.push({resolve})):Promise.resolve(byOwner[owner]);
  if(command==="memory_search")return Promise.resolve([]);
  return new Promise(()=>{});});
 const render=(ownerSessionId:string)=>root.render(createElement(JourneyOwnerPanel,{ownerSessionId,onClose:vi.fn()}));
 const search=async(q:string)=>{const input=root.container.querySelector<HTMLInputElement>('[aria-label="语义记忆查询"]')!;await dispatch(()=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!.call(input,q);input.dispatchEvent(new Event("input",{bubbles:true}));});await dispatch(()=>root.container.querySelector('form[aria-label="语义记忆"]')!.dispatchEvent(new Event("submit",{bubbles:true,cancelable:true})));};
 const failure=(kind:string)=>root.container.querySelector(`[data-index-failure="${kind}"]`);
 try{
  await render("A");
  expect(invoke).toHaveBeenCalledWith("memory_index_status",{ownerSessionId:"A"});
  expect(root.container.textContent).toContain("已索引 2 条");expect(failure("terminal")).toBeNull();expect(failure("retrying")).toBeNull();
  // Transient failure under the cap: visible as retrying, not as terminal.
  byOwner.A=status({indexed:1,queued:1,failed:1,failures:[{sourceId:"fact:f1",sourceKind:"confirmed_fact",attempt:1,terminal:false,error:"memory_query_transport_failed"}]});
  await search("q");
  expect(root.container.textContent).toContain("索引中 1 条");expect(failure("retrying")!.textContent).toContain("fact:f1");expect(failure("retrying")!.textContent).toContain("1/3");expect(failure("retrying")!.textContent).toContain("memory_query_transport_failed");expect(failure("terminal")).toBeNull();
  // Retry exhaustion: an alert naming the reason, the source and that it is not retried automatically.
  byOwner.A=status({indexed:1,failed:1,terminalFailed:1,failures:[{sourceId:"fact:f1",sourceKind:"confirmed_fact",attempt:3,terminal:true,error:"memory_query_credential_unavailable"}]});
  await search("q2");
  const alert=failure("terminal")!;expect(alert).not.toBeNull();expect(alert.getAttribute("role")).toBe("alert");
  expect(alert.textContent).toContain("重试 3 次后仍失败");expect(alert.textContent).toContain("fact:f1");expect(alert.textContent).toContain("memory_query_credential_unavailable");expect(failure("retrying")).toBeNull();
  // Owner switch: A's exhausted failure is never shown for B, neither while B's status loads nor after.
  await render("B");expect(failure("terminal")).toBeNull();expect(root.container.textContent).not.toContain("memory_query_credential_unavailable");
  await dispatch(()=>pendingB[0].resolve(status({indexed:5})));expect(failure("terminal")).toBeNull();expect(root.container.textContent).toContain("已索引 5 条");
  // A status load failure is stated, not rendered as a healthy index.
  vi.mocked(invoke).mockImplementation((command)=>command==="memory_index_status"?Promise.reject(new Error("db busy")):new Promise(()=>{}));
  await render("C");expect(root.container.textContent).toContain("索引状态不可用：Error: db busy");
 }finally{await root.unmount();}
});
