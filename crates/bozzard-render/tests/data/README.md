# Factory ordering certificate

`factory_ordering_envelopes.bin` contains one real 2,891-object Earth Factory
certificate that exhausted the former static hierarchy's 185,024-visit cap.
It was captured during the coordinated development diagnostic at runtime
`356b`, after adding movement slack and mandatory camera padding. It contains
only emitted ranks and envelope bits, without shader or object metadata.

All integers are little-endian `u32`. The 28-byte header stores the 8-byte
magic `BZBC0001`, object count, screen-space flag, and three camera-padding
float bits. Each subsequent 28-byte source-order record stores an emitted
rank, then six `f32` bits: minimum XYZ and maximum XYZ. The padding is already
included in the envelopes. File size is 80,976 bytes.

The CPU regression compares all inverted overlaps with brute force and the
pinned main X-axis sweep on identical bits. It bounds query visits plus source
activation updates by the unchanged 64N cap; constructor partitioning remains
bounded O(N log N) work outside that traversal cap. Neighbor enumeration and
node/update visits are distinct work counts, not a CPU timing comparison.
