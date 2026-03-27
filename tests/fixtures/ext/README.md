# EXT fixture notes

`minimal.ext` is a synthetic file made of:
- 8-byte ASCII header `MRPGCMAP`
- one ARM instruction payload `E3A00001` (`mov r0, #1` in little-endian bytes)

Real-sample analysis in tests targets `D:\opt\rust\vmrp\mrc\asm\cfunction.ext` and assumes:
- the file starts with the same 8-byte `MRPGCMAP` header
- execution enters at byte offset 8
- the first observed entry words are:
  - `E92D4038`
  - `E59F410C`
  - `E08F4004`
  - `E5141008`
  - `E3500001`
  - `E5912064`
