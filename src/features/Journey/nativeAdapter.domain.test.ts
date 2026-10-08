import {beforeEach,expect,it,vi} from "vitest";import {invoke} from "@tauri-apps/api/core";import {loadJourneyDomainSnapshot,loadJourneyBoundaryPreview} from "./nativeAdapter";vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));beforeEach(()=>vi.mocked(invoke).mockReset());
const domain={revision:0,snapshot:{session_id:"A",active_branch_id:"main",tasks:{},branches:{},checkpoints:{},facts:{},reviews:{}}};
const anchor={ownerSessionId:"A",sourceSessionId:"A",branchId:"main",durableMessageId:"one",boundaryVersion:1,boundaryHash:"hash"};
it("accepts the exact registered owner response including revision zero",async()=>{vi.mocked(invoke).mockResolvedValue(domain);expect(await loadJourneyDomainSnapshot("A")).toEqual(domain);});
for(const revision of [NaN,Infinity,-1,1.5,Number.MAX_SAFE_INTEGER+1])it("rejects invalid durable revision "+String(revision),async()=>{vi.mocked(invoke).mockResolvedValue({...domain,revision});await expect(loadJourneyDomainSnapshot("A")).rejects.toThrow("Journey domain response invalid");});
for(const tasks of [null,[],"bad"])it("rejects malformed domain collection "+String(tasks),async()=>{vi.mocked(invoke).mockResolvedValue({...domain,snapshot:{...domain.snapshot,tasks}});await expect(loadJourneyDomainSnapshot("A")).rejects.toThrow("Journey domain response invalid");});
it("rejects mismatched owner with explicit boundary error",async()=>{vi.mocked(invoke).mockResolvedValue({...domain,snapshot:{...domain.snapshot,session_id:"B"}});await expect(loadJourneyDomainSnapshot("A")).rejects.toThrow("Journey domain owner mismatch");});
it("rejects invalid requesting owner before IPC",async()=>{await expect(loadJourneyDomainSnapshot("")).rejects.toThrow("Journey domain owner invalid");expect(invoke).not.toHaveBeenCalled();});
it("rejects owner-changing preview request before native IPC",async()=>{await expect(loadJourneyBoundaryPreview("A",{...anchor,ownerSessionId:"B"})).rejects.toThrow("Journey boundary request mismatch");expect(invoke).not.toHaveBeenCalled();});
for(const boundaryVersion of [-1,NaN,Infinity,1.5,Number.MAX_SAFE_INTEGER+1])it("rejects invalid durable boundary revision "+String(boundaryVersion),async()=>{await expect(loadJourneyBoundaryPreview("A",{...anchor,boundaryVersion})).rejects.toThrow("Journey boundary request mismatch");expect(invoke).not.toHaveBeenCalled();});
for(const boundaryVersion of [0,2,7])it("accepts actual native durable boundary revision "+boundaryVersion,async()=>{const selected={...anchor,boundaryVersion};vi.mocked(invoke).mockResolvedValue({ownerSessionId:"A",anchor:selected,prefixHash:"prefix",records:[]});expect((await loadJourneyBoundaryPreview("A",selected)).anchor).toEqual(selected);});

it("explicit durable open denies backend snapshot without readable owner capability",async()=>{
 vi.mocked(invoke).mockResolvedValue({schemaVersion:1,ownerSessionId:"owner",revision:1,nodes:[],edges:[],capabilities:{read:false,fork:false,import:false,mutate:false}});
 const {openDurableJourneyOwner}=await import("./nativeAdapter");await expect(openDurableJourneyOwner("owner")).rejects.toThrow();
});
