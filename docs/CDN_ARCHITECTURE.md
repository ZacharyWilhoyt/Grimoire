# Future hybrid media distribution

The Source Agent preview has one serving location per representation. It
supports an outbound coordinator relay. The relay carries bytes through
Grimoire and has no replica, cache or placement role.

A future hybrid CDN can combine on-demand distribution with proactive
placement for sustained demand. Placement should use measured throughput,
latency, available storage and bandwidth, expected demand, and source
ownership policy. An exact content digest identifies a particular byte
representation. Logical media metadata can group different encodes without
asserting byte equivalence.

Cache and replication rights must be explicit and independent from
discovery, playback, acquisition and any future remote transcoding right.
Placement needs consent from both the source owner and the storage operator.
Any future scheduler should track provenance, verification, quota, expiry
and revocation. Cooperative caches can stop serving or delete copies after
revocation. Plaintext already delivered to a client cannot reliably be
recalled.

Automatic replication, cache exchange and distributed transcoding are
deferred.
