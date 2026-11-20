package dev.sunriselab.parity;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.core.SectionPos;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.chunk.DataLayer;
import net.minecraft.world.level.chunk.LightChunk;
import net.minecraft.world.level.chunk.LightChunkGetter;
import net.minecraft.world.level.lighting.LevelLightEngine;

/**
 * Replays tools/light_storage_scenario.txt on the real LevelLightEngine, as a client drives it
 * (ClientPacketListener: updateSectionStatus per section, readSectionList, setLightEnabled; forgetting a
 * column is queueLightRemoval), and prints the sky and block light of every read.
 */
public final class VanillaLightStorageOracle {
    private static final int MIN_SECTION = -4;
    private static final int SECTIONS = 24;

    private static LevelLightEngine engine(boolean sky) {
        BlockGetter level = new BlockGetter() {
            public net.minecraft.world.level.block.entity.BlockEntity getBlockEntity(BlockPos pos) { return null; }
            public net.minecraft.world.level.block.state.BlockState getBlockState(BlockPos pos) { return null; }
            public net.minecraft.world.level.material.FluidState getFluidState(BlockPos pos) { return null; }
            public int getHeight() { return SECTIONS * 16; }
            public int getMinY() { return MIN_SECTION * 16; }
        };
        LightChunkGetter getter = new LightChunkGetter() {
            public LightChunk getChunkForLighting(int x, int z) { return null; }
            public BlockGetter getLevel() { return level; }
        };
        return new LevelLightEngine(getter, true, sky);
    }

    private static int value(String spec, int x, int y, int z) {
        int n = Integer.parseInt(spec.substring(1));
        return spec.charAt(0) == 'U' ? n : (x + 2 * z + 3 * y + n) & 15;
    }

    private static DataLayer layer(String spec) {
        DataLayer layer = new DataLayer();
        for (int y = 0; y < 16; y++)
            for (int z = 0; z < 16; z++)
                for (int x = 0; x < 16; x++)
                    layer.set(x, y, z, value(spec, x, y, z));
        return layer;
    }

    public static void main(String[] args) throws Exception {
        List<String> out = new ArrayList<>();
        LevelLightEngine engine = null;
        for (String line : Files.readAllLines(Path.of(args[0]))) {
            if (line.isBlank() || line.startsWith("#")) continue;
            String[] f = line.split(" ");
            switch (f[0]) {
                case "case" -> {
                    engine = engine(f.length < 3 || !f[2].equals("nosky"));
                    out.add("== " + f[1]);
                }
                case "column" -> {
                    int cx = Integer.parseInt(f[1]), cz = Integer.parseInt(f[2]);
                    List<Integer> nonAir = new ArrayList<>();
                    for (String s : f[3].substring("nonair=".length()).split(","))
                        if (!s.isEmpty()) nonAir.add(Integer.parseInt(s));
                    ChunkPos pos = new ChunkPos(cx, cz);
                    for (int y = MIN_SECTION; y < MIN_SECTION + SECTIONS; y++)
                        engine.updateSectionStatus(SectionPos.of(pos, y), !nonAir.contains(y));
                    for (String s : f[4].substring("stored=".length()).split(",")) {
                        String[] p = s.split(":");
                        String[] k = p[1].split("/");
                        SectionPos at = SectionPos.of(cx, Integer.parseInt(p[0]), cz);
                        engine.queueSectionData(LightLayer.SKY, at, layer(k[0]));
                        engine.queueSectionData(LightLayer.BLOCK, at, layer(k[1]));
                    }
                    engine.setLightEnabled(pos, true);
                    engine.runLightUpdates();
                }
                case "drop" -> {
                    ChunkPos pos = new ChunkPos(Integer.parseInt(f[1]), Integer.parseInt(f[2]));
                    engine.setLightEnabled(pos, false);
                    for (int y = MIN_SECTION - 1; y < MIN_SECTION + SECTIONS + 1; y++) {
                        engine.queueSectionData(LightLayer.BLOCK, SectionPos.of(pos, y), null);
                        engine.queueSectionData(LightLayer.SKY, SectionPos.of(pos, y), null);
                    }
                    for (int y = MIN_SECTION; y < MIN_SECTION + SECTIONS; y++)
                        engine.updateSectionStatus(SectionPos.of(pos, y), true);
                    engine.runLightUpdates();
                }
                case "read" -> {
                    BlockPos pos = new BlockPos(Integer.parseInt(f[1]), Integer.parseInt(f[2]), Integer.parseInt(f[3]));
                    out.add(line + " -> " + engine.getLayerListener(LightLayer.SKY).getLightValue(pos) + " "
                        + engine.getLayerListener(LightLayer.BLOCK).getLightValue(pos));
                }
                default -> throw new IllegalArgumentException(line);
            }
        }
        Files.write(Path.of(args[1]), out);
    }
}
