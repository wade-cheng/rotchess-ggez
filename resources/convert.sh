#!/usr/bin/env bash
for i in $(ls pieces); do 
    rsvg-convert -w 200 -h 200 pieces/$i > pieces_png/$(basename $i .svg).png; 
done
