import {it,expect} from "vitest";
import {parseContextImportCardResult} from "../cardParsers";
it("native durable snapshot camelCase contract renders without fabricated input metadata",()=>{
 const result={snapshotId:"durable-snapshot",targetSessionId:"owner",sourceKind:"session",sourceId:"owner",namespace:"session:owner",title:"Native Selected Context",tokenEstimate:123,pinned:true,snippet:"explicit selected prefix",anchor:{ownerSessionId:"owner",sourceSessionId:"owner",branchId:"session:owner",durableMessageId:"end",boundaryVersion:3,boundaryHash:"sha256"},prefixHash:"prefix-sha256"};
 expect(parseContextImportCardResult({},result)).toEqual({snapshotId:"durable-snapshot",sourceKind:"session",sourceId:"owner",namespace:"session:owner",title:"Native Selected Context",tokenEstimate:123,pinned:true});
});
