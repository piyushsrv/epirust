/*
 * EpiRust
 * Copyright (c) 2020  ThoughtWorks, Inc.
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 */
import React, { useContext, useRef, useState, useEffect } from 'react';
import { GridContext } from './index'

export default function GridAreas({ areaDimensions }) {

    const { cellDimension, lineWidth, canvasDimension, size } = useContext(GridContext);

    const gridCanvasAreas = useRef(null);
    const [areasCanvasContext, setAreasCanvasContext] = useState(null);

    useEffect(() => {
        if (!gridCanvasAreas)
            return

        setAreasCanvasContext(gridCanvasAreas.current.getContext("2d"));

    }, [gridCanvasAreas])

    useEffect(() => {
        if (!areasCanvasContext)
            return

        function updateAreaColor(areaDimensions, x, y) {
            let area;
            for (var i = 0; i < areaDimensions.length; i++) {
                if (isWithinArea(areaDimensions[i], x, y)) {
                    area = areaDimensions[i];
                    break;
                }
            }

            const color = area ? area.color : "#ccc";

            if (areasCanvasContext.fillStyle !== color) {
                areasCanvasContext.fillStyle = color;
            }
        }

        for (let x = 0; x < size; x++) {
            for (let y = 0; y < size; y++) {
                updateAreaColor(areaDimensions, x, y);
                areasCanvasContext.fillRect((x * cellDimension) + lineWidth / 2, (y * cellDimension) + lineWidth / 2, cellDimension, cellDimension)
            }
        }
    }, [areasCanvasContext, size, cellDimension, lineWidth, areaDimensions])

    return (
        <canvas ref={gridCanvasAreas} id="grid-canvas" width={canvasDimension} height={canvasDimension} style={{ position: "absolute", zIndex: 1 }} />
    )
}

function isWithinArea(area, currentX, currentY) {

    return (currentX >= area.start_offset.x && currentY >= area.start_offset.y
        && currentX < area.end_offset.x && currentY < area.end_offset.y);
}