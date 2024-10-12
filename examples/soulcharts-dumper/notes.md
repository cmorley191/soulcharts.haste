input data:
- moving positions of:
  - players
  - lane creeps
  - soul orbs
  - player projectiles (grenades, mcginnis turret, seven ball, talon owl, etc.)
  - soul urn
  - rejuvinator?
  - walker?! no. it's so big, let's just assume all misses are aiming for something else
    - same with patron
- fixed positions of:
  - neutrals
  - objectives
  - mid boss
  - vending machines
  - boxes & golden urns
- aiming vectors of players & talon owl
  - angular distances from aiming vectors to potential targets
    - provide distance, as well as horizontal distance and vertical distance
- damages (308)
- bullets fired / bullets impact (??? / ???)
- reload periods
- melee swings
- ability casts



assumptions
- one tick message per packet (verified via python)
- one entity message per packet (verified via python)
- ordering of important frames/messages:
  - Frames with message 4:           net_Tick
  - Frame with Message  40:          svc_ServerInfo (1 per replay)
    - provides tick_interval
  - Frames with message 4:           net_Tick
  - Frame    3:                      DEM_SyncTick (1 per replay)
  - Per frame:
    - Messages 300s & 319s (mixed):  k_EUserMsg_Damage & k_EUserMsg_BulletHit
    - Message  4:                    net_Tick (1 per frame)
    - Message  55:                   svc_PacketEntities (1 per frame)
    - Messages 450s & 461s (mixed):  GE_FireBullets & GE_BulletImpact
    - 
    - NOT PRESENT: Message 323: k_EUserMsg_BulletHit

- one entity "update" per entity message (soulcharts-check-one-update-per-packet)
  - either:
    - CREATE
    - DELETE/LEAVE
    - CREATE DELETE/LEAVE
    - DELETE/LEAVE CREATE
    - UPDATE
once i know that...
i'll know that each entity has a single *before* and *after* state for each packet.
which means I just track all the after states, associated with the tick




4 (net_Tick)
6 (net_SetConVar)
7 (net_SignonState)
8 (net_SpawnGroup_Load)
11 (net_SpawnGroup_SetCreationTick)
40 (svc_ServerInfo)
42 (svc_ClassInfo)
44 (svc_CreateStringTable)
45 (svc_UpdateStringTable)
46 (svc_VoiceInit)
51 (svc_ClearAllStringTables)
 55 (svc_PacketEntities)
62 (svc_HLTVStatus)
 76 (svc_UserCmds)
145 (UM_ParticleManager)
 166 (UM_PlayResponseConditional)
205 (GE_Source1LegacyGameEventList)
207 (GE_Source1LegacyGameEvent)
208 (GE_SosStartSoundEvent)
209 (GE_SosStopSoundEvent)
210 (GE_SosSetSoundEventParams)
212 (GE_SosSetLibraryStackFields)
 300 (k_EUserMsg_Damage)
303 (k_EUserMsg_MapPing)
308 (k_EUserMsg_TriggerDamageFlash)
312 (k_EUserMsg_ChatWheel)
314 (k_EUserMsg_ChatMsg)
317 (k_EUserMsg_ChatEvent)
 319 (k_EUserMsg_HeroKilled)
332 (k_EUserMsg_MapLine)
 338 (k_EUserMsg_AbilityNotify)
340 (k_EUserMsg_ParticipantStartSoundEvent)
343 (k_EUserMsg_ParticipantSetSoundEventParams)
346 (k_EUserMsg_GameOver)
347 (k_EUserMsg_BossKilled)
400 (TE_EffectDispatchId)
 450 (GE_FireBullets)
 461 (GE_BulletImpact)
 500 (k_EEntityMsg_BreakablePropSpawnDebris)