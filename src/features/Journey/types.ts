export interface DurableAnchor { ownerSessionId:string; sourceSessionId:string; branchId:string; durableMessageId:string; durableTurnId?:string|null; boundaryVersion:number; boundaryHash:string; }
export type MarkerKind='decision'|'pivot'|'abandon'|'artifact'|'finding'|'question'|'verify'|'handoff'|'interrupt';
export interface CanonicalNode { id:string; title:string; timestamp:number; laneId:string; kind:MarkerKind; anchor:DurableAnchor; providerVisible?:boolean; }
export interface CanonicalEdge { from:string; to:string; kind:'forked_from'|'resumed_from'|'compacted_to'; evidenceId:string; }
export interface JourneySnapshot {schemaVersion:1;ownerSessionId:string;revision:number;nodes:CanonicalNode[];edges:CanonicalEdge[];capabilities:{read:boolean;fork:boolean;import:boolean;mutate:boolean};}
