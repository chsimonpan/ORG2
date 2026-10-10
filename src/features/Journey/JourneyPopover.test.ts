// @vitest-environment jsdom
import {it,expect,vi} from "vitest";import {createElement} from "react";
import {createSmokeRoot,dispatch} from "@src/test/reactSmokeHarness";
import {JourneyOwnerPanel,JourneyPopover} from "./JourneyPopover";import {loadJourneySnapshot,openDurableJourneyOwner} from "./nativeAdapter";import type {JourneySnapshot} from "./types";
vi.mock("./nativeAdapter",()=>({openDurableJourneyOwner:vi.fn(),loadJourneySnapshot:vi.fn(),loadJourneyBoundaryPreview:vi.fn(),loadJourneyDomainSnapshot:vi.fn(()=>new Promise(()=>{}))}));
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));
const snap=(owner:string,id:string):JourneySnapshot=>({schemaVersion:1,ownerSessionId:owner,revision:1,nodes:[{id,title:id,timestamp:1000,laneId:`session:${owner}`,kind:"question",anchor:{ownerSessionId:owner,sourceSessionId:owner,branchId:`session:${owner}`,durableMessageId:id,boundaryVersion:1,boundaryHash:`hash-${id}`}}],edges:[],capabilities:{read:true,fork:false,import:false,mutate:false}});
it("mounted canonical entry rejects deferred A1 success/error after A-B-A and selects an explicit historical endpoint",async()=>{
 let a1!:(s:JourneySnapshot)=>void,a2!:(s:JourneySnapshot)=>void,bad!:(e:Error)=>void;
 vi.mocked(loadJourneySnapshot).mockReset().mockImplementationOnce(()=>new Promise(r=>a1=r)).mockImplementationOnce(()=>new Promise((_,r)=>bad=r)).mockImplementationOnce(()=>new Promise(r=>a2=r)).mockImplementation(()=>new Promise(()=>{}));
 const root=createSmokeRoot();try{
 const render=(ownerSessionId:string)=>root.render(createElement(JourneyOwnerPanel,{ownerSessionId,onClose:vi.fn()}));
 await render("A");await render("B");await render("A");await dispatch(()=>a1(snap("A","STALE-A")));await dispatch(()=>bad(new Error("STALE-B")));expect(root.container.textContent).not.toContain("STALE");
 await dispatch(()=>a2(snap("A","EARLIER-A")));expect(root.container.textContent).toContain("EARLIER-A");const select=root.container.querySelector("select")!;await dispatch(()=>{select.value="EARLIER-A";select.dispatchEvent(new Event("change",{bubbles:true}));});expect(root.container.querySelector("[data-testid=journey-explicit-endpoint]")?.textContent).toContain("hash-EARLIER-A");
 await render("B");expect(root.container.querySelector("[data-testid=journey-explicit-endpoint]")).toBeNull();expect(root.container.textContent).not.toContain("EARLIER-A");
 }finally{await root.unmount();}
});

it("owner switch while durable open pending releases old gesture state",async()=>{
 vi.mocked(openDurableJourneyOwner).mockReset();vi.mocked(openDurableJourneyOwner).mockImplementation(()=>new Promise(()=>{}));const root=createSmokeRoot();
 try{await root.render(createElement(JourneyPopover,{ownerSessionId:"A"}));await dispatch(()=>(root.container.querySelector("button[aria-label='Session Journey']") as HTMLElement).click());expect((root.container.querySelector("button[aria-label='Session Journey']") as HTMLButtonElement).disabled).toBe(true);await root.render(createElement(JourneyPopover,{ownerSessionId:"B"}));expect((root.container.querySelector("button[aria-label='Session Journey']") as HTMLButtonElement).disabled).toBe(false);expect(root.container.querySelector("[role=dialog]")).toBeNull();}finally{await root.unmount();}
});

it("production Journey selection opens and activates the physical child session tab",async()=>{
 const {invoke}=await import("@tauri-apps/api/core");const {Provider}=await import("jotai");const {createInstrumentedStore}=await import("@src/util/core/state/instrumentedStore");const store=createInstrumentedStore();const {activeChatPanelTabAtom}=await import("@src/store/chatPanel/chatPanelTabsState");
 vi.mocked(loadJourneySnapshot).mockReset().mockResolvedValue(snap("A","answer"));vi.mocked(invoke).mockResolvedValue({childSessionId:"A-history-child",parentSessionId:"A"});
 const root=createSmokeRoot(),close=vi.fn();try{await root.render(createElement(Provider,{store},createElement(JourneyOwnerPanel,{ownerSessionId:"A",onClose:close})));await dispatch(()=>{});
 const select=root.container.querySelector("select")!;await dispatch(()=>{select.value="answer";select.dispatchEvent(new Event("change",{bubbles:true}));});
 const button=Array.from(root.container.querySelectorAll("button")).find(b=>b.textContent==="Continue in new runtime from exact endpoint")!;expect(button.disabled).toBe(false);await dispatch(()=>button.click());
 expect(invoke).toHaveBeenCalledWith("historical_runtime_fork",{ownerSessionId:"A",request:{anchor:snap("A","answer").nodes[0].anchor,intentId:expect.any(String)}});expect(store.get(activeChatPanelTabAtom)).toMatchObject({type:"session",sessionId:"A-history-child"});expect(root.container.querySelector("[aria-label=\"Historical runtime fork\"]")?.textContent).not.toContain("Error");expect(close).toHaveBeenCalledTimes(1);
 }finally{await root.unmount();}
});
