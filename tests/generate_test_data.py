import os
from shutil import copyfile

TORRENT_A_LEN = 1000001

PIECE_LENGTH = 32768

NUM_PIECES = int(TORRENT_A_LEN / PIECE_LENGTH) + (TORRENT_A_LEN % PIECE_LENGTH > 0)

CHUNK_1_START = 0
CHUNK_1_END = int(NUM_PIECES / 3) * PIECE_LENGTH
CHUNK_2_START = CHUNK_1_END
CHUNK_2_END = int(2 * NUM_PIECES / 3) * PIECE_LENGTH
CHUNK_3_START = CHUNK_2_END
CHUNK_3_END = PIECE_LENGTH

if not os.path.exists('tests/data/generated'):
    os.makedirs('tests/data/generated')

with open('tests/data/generated/torrent_a_data', 'w') as f:
    s = ''
    for i in range(TORRENT_A_LEN):
        s += chr(ord('a') + (i % 26))
    f.write(s)

with open('tests/data/generated/torrent_b_data', 'w') as f:
    s = ''
    for i in range(1000001):
        s += chr(ord('z') - (i % 26))
    f.write(s)

with open('tests/data/generated/torrent_c_data', 'w') as f:
    s = ''
    for i in range(1000001):
        s += chr(ord('0') + (i % 10))
    f.write(s)

with open('tests/data/generated/torrent_a_data_chunk_1', 'w') as f:
    s = ''
    for i in range(TORRENT_A_LEN):
        c = chr(ord('a') + (i % 26)) if i >= CHUNK_1_START and i < CHUNK_1_END else '_'
        s += c
    f.write(s)

with open('tests/data/generated/torrent_a_data_chunk_2', 'w') as f:
    s = ''
    for i in range(TORRENT_A_LEN):
        c = chr(ord('a') + (i % 26)) if i >= CHUNK_2_START and i < CHUNK_2_END else '_'
        s += c
    f.write(s)

with open('tests/data/generated/torrent_a_data_chunk_3', 'w') as f:
    s = ''
    for i in range(TORRENT_A_LEN):
        c = chr(ord('a') + (i % 26)) if i >= CHUNK_3_START and i < CHUNK_3_END else '_'
        s += c
    f.write(s)

if not os.path.exists('tests/data/generated/torrent_d/b'):
    os.makedirs('tests/data/generated/torrent_d/b')
copyfile('tests/data/generated/torrent_a_data', 'tests/data/generated/torrent_d/torrent_a_data')
copyfile('tests/data/generated/torrent_b_data', 'tests/data/generated/torrent_d/b/torrent_b_data')

if not os.path.exists('tests/data/generated/torrent_e/a'):
    os.makedirs('tests/data/generated/torrent_e/a')
copyfile('tests/data/generated/torrent_a_data', 'tests/data/generated/torrent_e/a/torrent_a_data')
copyfile('tests/data/generated/torrent_b_data', 'tests/data/generated/torrent_e/torrent_b_data')
