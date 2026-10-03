import argparse
from pathlib import Path

from media_contract import prepare_output, write_json
from memory_extension import computer_finale, memory_picture, prepare
from trailer_scenes import load_brand_frames


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Prepare sample-bound Memory narration insertion and finale stills')
    for name in ('original-timing', 'original-audio', 'original-alignment', 'memory-audio',
                 'memory-alignment', 'memory-metadata', 'music', 'music-credit', 'output',
                 'brand-frames', 'font'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    prepare(args.original_timing, args.original_audio, args.original_alignment, args.memory_audio,
            args.memory_alignment, args.memory_metadata, args.music, args.music_credit, args.output)
    brand = load_brand_frames(args.brand_frames)['images'][-1]
    computer_finale(args.font, brand).save(args.output / 'computer-finale.png')
    print(f'Prepared Memory episode and computer finale: {args.output}', flush=True)
