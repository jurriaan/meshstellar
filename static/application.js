function getRelativeTimeString(date, lang = navigator.language) {
    // Allow dates or times to be passed
    const timeMs = typeof date === "number" ? date : date.getTime();

    if (!isFinite(timeMs)) {
        return 'unknown';
    }

    // Get the amount of seconds between the given date and now
    const deltaSeconds = Math.round((timeMs - Date.now()) / 1000);

    // Array reprsenting one minute, hour, day, week, month, etc in seconds
    const cutoffs = [59, 3600, 86400, 86400 * 7, 86400 * 30, 86400 * 365, Infinity];

    // Array equivalent to the above but in the string representation of the units
    const units = ["second", "minute", "hour", "day", "week", "month", "year"];

    // Grab the ideal cutoff unit
    const unitIndex = cutoffs.findIndex(cutoff => cutoff > Math.abs(deltaSeconds));

    if (unitIndex == 0) return 'just now';

    // Get the divisor to divide from the seconds. E.g. if our unit is "day" our divisor
    // is one day in seconds, so we can divide our seconds by this to get the # of days
    const divisor = unitIndex ? cutoffs[unitIndex - 1] : 1;

    // Intl.RelativeTimeFormat do its magic
    const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
    return rtf.format(Math.floor(deltaSeconds / divisor), units[unitIndex]);
}

function applyRelativeDateTime(node) {
    for (const relativeTime of node.querySelectorAll('time.relative[datetime]')) {
        const date = Date.parse(relativeTime.attributes.datetime.value);
        const timeMs = typeof date === "number" ? date : date.getTime();

        relativeTime.dataset.time = (timeMs / 1000).toFixed(0);
        relativeTime.innerText = getRelativeTimeString(date);
    }
}

let selectedNode = null;
let map = null;
const nodeGeoJSON = { type: 'FeatureCollection', features: [] };
const nodeFeaturesById = {};
const neighborsGeoJSON = { type: 'FeatureCollection', features: [] };
const emptyFeatureCollection = { type: 'FeatureCollection', features: [] };

function selectNode(node) {
    const nodeEl = node ? htmx.find('#' + node?.id) : null;
    selectedNode?.classList?.remove('selected')

    map.getSource('positions').setData(emptyFeatureCollection);

    if (selectedNode == nodeEl || !node?.id) {
        selectedNode = null;
    } else {
        selectedNode = nodeEl;
        let geojson = htmx.find(nodeEl, '[data-geojson]')?.dataset?.geojson;
        if (geojson) {
            geojson = JSON.parse(geojson);

            if(geojson.geometry && geojson.geometry.coordinates.length > 0) {
                map.flyTo({
                    center: geojson.geometry.coordinates,
                    zoom: 12
                });
            }
        }
        nodeEl?.classList?.add('selected')
        map.getSource('positions').setData(`/node/${node.dataset.nodeId}/positions.geojson`);
    }
    _updateNodeGeoJSON();
}

function debounce(fn, delay) {
    var timer = null;
    return function () {
        var context = this, args = arguments;
        clearTimeout(timer);
        timer = setTimeout(function () {
            fn.apply(context, args);
        }, delay);
    };
}

let mapCentered = false;
function centerMap() {
    const features = nodeGeoJSON?.features;
    if (!mapCentered && features?.length > 0 && features[0].geometry && features[0].geometry.coordinates) {
        mapCentered = true;
        const latestTimestamp = features[0].properties.last_rx_time;
        const coordinates = features.filter((point) => latestTimestamp - point.properties.last_rx_time < 86400).map((point) => point.geometry.coordinates);
        const bounds = coordinates.reduce((bounds, coord) => {
            return bounds.extend(coord);
        }, new maplibregl.LngLatBounds(coordinates[0], coordinates[0]));

        map.fitBounds(bounds, {
            padding: 150
        });
    }
}

let loadingIsFinished = false;
function loadingFinished() {
    if (!loadingIsFinished) {
        loadingIsFinished = true;

        setTimeout(() => {
            htmx.find('#sidebar')?.removeAttribute("hidden");

            centerMap();
        }, 500);

        requestAnimationFrame(animate);
    }
}

function _updateNodeGeoJSON() {
    _refreshOnlineState();

    const nodeList = htmx.find('.node-list');
    const showOld = nodeList.classList.contains('show-old');
    const nodes = Array.from(htmx.findAll(nodeList, 'li'));
    const selectedIndex = nodes.findIndex((node) => node.classList.contains('selected'));
    if (selectedIndex > 0) nodes.unshift(nodes.splice(selectedIndex, 1)[0]);

    const nodeData = nodes
        .map((node) => {
            let neighbors = htmx.find(node, '[data-neighbor-json]')?.dataset?.neighborJson;
            if (neighbors) neighbors = JSON.parse(neighbors);
            let geojson = htmx.find(node, '[data-geojson]')?.dataset?.geojson;
            if (geojson) geojson = JSON.parse(geojson);

            return {
                id: node.dataset.nodeId,
                neighbors: neighbors,
                geojson: geojson,
                selected: node.classList.contains('selected'),
                isOld: node.classList.contains('is-old')
            }
        })
        .filter((node) => node.selected || (showOld || !node.isOld));

    nodeGeoJSON.features = nodeData.map((nodeData) => nodeData.geojson).filter((geojson) => geojson);
    nodeGeoJSON.features.forEach((node) => {
        nodeFeaturesById[node.properties.id] = node;
    });

    const neighbors = nodeData
        .filter((node) => node.neighbors && node.neighbors.length > 0 && (selectedIndex == -1 || node.selected))
        .flatMap((node) => {
            return node.neighbors.map((neighbor) => {
                const nodes = [node.id, neighbor.neighbor].sort()

                return {
                    a: nodes[0],
                    b: nodes[1],
                    snr: neighbor.snr,
                    timestamp: neighbor.timestamp
                };
            });
        })
        .filter((neighbor) => nodeFeaturesById[neighbor.a] && nodeFeaturesById[neighbor.b])
        .sort((neighborA, neighborB) => neighborB.timestamp - neighborA.timestamp);

    const neighborSet = new Set();
    const neighborFeatures = [];
    neighbors.forEach((neighbor) => {
        const key = neighbor.a + "-" + neighbor.b;

        if (neighborSet.has(key)) return;
        neighborSet.add(key);

        neighborFeatures.push({
            "type": "Feature",
            "geometry": {
                "type": "LineString",
                "coordinates": [
                    nodeFeaturesById[neighbor.a].geometry.coordinates,
                    nodeFeaturesById[neighbor.b].geometry.coordinates,
                ]
            },
            "properties": {
                "id": key,
                "snr": neighbor.snr
            }
        });
    });

    neighborsGeoJSON.features = neighborFeatures;

    map?.getSource('nodes')?.setData(nodeGeoJSON);
    map?.getSource('neighbors')?.setData(neighborsGeoJSON);

    if (!loadingIsFinished) {
        loadingFinished();
    }
}
const updateNodeGeoJSON = debounce(_updateNodeGeoJSON, 500);

const recentNodes = new Set();
const lastRxPerNode = {};

function animate() {
    const curDate = (Date.now() / 1000).toFixed(0);

    for (const [nodeId, lastRxTime] of Object.entries(lastRxPerNode)) {
        const age = curDate - lastRxTime;
        const recentTx = age > 0 && age < 30;

        if (recentTx || recentNodes.has(nodeId)) {
            if (!recentTx) {
                recentNodes.delete(nodeId);

                map.setFeatureState({ source: 'nodes', layers: ['node-symbols'], id: nodeId }, { 'age': null });
            } else {
                recentNodes.add(nodeId);

                map.setFeatureState({ source: 'nodes', layers: ['node-symbols'], id: nodeId }, { 'age': age });
            }
        }
    }

    requestAnimationFrame(animate);
}

function _refreshOnlineState() {
    const sidebar = htmx.find('#sidebar');
    applyRelativeDateTime(sidebar);

    const nodes = htmx.findAll('.node-list li');
    const curDate = (Date.now() / 1000).toFixed(0);

    for (const node of nodes) {
        const timeNode = htmx.find(node, 'time.relative[datetime]');
        const nodeTime = timeNode?.dataset?.time;
        if (nodeTime) {
            const nodeId = node.dataset.nodeId;
            lastRxPerNode[nodeId] = nodeTime;
            const age = Math.abs(curDate - nodeTime);
            const isOnline = age < 15 * 60;
            node.dataset.isOnline = isOnline;
            const isOld = age > 86400 * 1.5;
            node.dataset.isOld = isOld;

            if (isOnline) {
                node.classList.remove('is-offline');
                node.classList.add('is-online');
            } else {
                node.classList.remove('is-online');
                node.classList.add('is-offline');
            }

            if (isOld) {
                node.classList.add('is-old');
            } else {
                node.classList.remove('is-old');
            }
        }
    }
}

const refreshOnlineState = debounce(_refreshOnlineState, 250);

function refreshMap() {
    updateNodeGeoJSON();
}

function handleSseMessage(event) {
    const eventType = event?.detail?.type;
    if (eventType == 'update-node') {
        updateNodeGeoJSON();
    } else if (eventType == 'mesh-packet') {
        refreshOnlineState();
    }
}

function loadMap() {
    const protocol = new pmtiles.Protocol();
    maplibregl.addProtocol("pmtiles", protocol.tile);

    const mapContainer = htmx.find('#map');

    map = new maplibregl.Map({
        container: mapContainer,
        style: '/map/style.json'
    });

    let image = map.loadImage('/static/node-symbol.png');

    map.once("load", async () => {
        map.addImage('node-symbol', (await image).data, { sdf: true });
    });

    window.map = map;

    return map;
}

document.addEventListener('DOMContentLoaded', () => {
    let internalApi = undefined;

    htmx.defineExtension('custom', {
        init: function (api) {
            internalApi = api;
        },
        transformResponse: function (text, xhr, elt) {
            const fragment = internalApi.makeFragment(text);

            applyRelativeDateTime(fragment);

            const swapAttr = elt.getAttribute('hx-swap');
            if (swapAttr == 'afterbegin' || swapAttr == 'beforeend') {
                const elements = htmx.findAll(fragment, "[hx-swap-oob=if-exists]");

                for (const element of elements) {
                    const selector = '#' + element.id;
                    const existingElement = htmx.find(selector);

                    if (!!existingElement) {
                        // Move element to the desired spot and swap the HTML
                        existingElement.parentNode.insertAdjacentElement(swapAttr, existingElement);
                        element.setAttribute('hx-swap-oob', 'innerHTML');
                    } else {
                        // Just create the element
                        element.removeAttribute('hx-swap-oob');
                    }
                }
            }

            const htmlContent = [].map.call(fragment.childNodes, x => x.outerHTML).join('')
            return htmlContent;
        }
    });

    loadMap();
    document.body.addEventListener("htmx:sseMessage", handleSseMessage);
    document.body.addEventListener("refreshMap", refreshMap);
    setInterval(function () { refreshMap() }, 30000);

    const resizer = document.querySelector("#resizer");
    // Listening for both mousedown and touchstart events
    resizer.addEventListener("mousedown", initResize, false);
    resizer.addEventListener("touchstart", initResize, false);
})


// Unified event handler to remove event listeners
function removeEvents() {
    document.removeEventListener("mousemove", resize, false);
    document.removeEventListener("mouseup", removeEvents, false);
    // Touch events
    document.removeEventListener("touchmove", resize, false);
    document.removeEventListener("touchend", removeEvents, false);
}

// Function to start listening to resize events
function initResize(event) {
    // Prevent default behavior to avoid any potential conflict with touch events
    event.preventDefault();

    document.addEventListener("mousemove", resize, false);
    document.addEventListener("mouseup", removeEvents, false);

    // Adding touch events
    document.addEventListener("touchmove", resize, false);
    document.addEventListener("touchend", removeEvents, false);
}

// Updated resize function to handle both touch and mouse events
function resize(e) {
    // Determine if this is a touch event or a mouse event and act accordingly
    const sidebar = document.getElementById('sidebar');
    const clientX = e.type.includes('touch') ? e.touches[0].clientX : e.clientX;
    if (clientX > 50) {
        sidebar.style.width = `${clientX}px`;
        sidebar.classList.remove('collapsed');
    } else {
        sidebar.style.width = 'auto';
        sidebar.classList.add('collapsed');
    }
}
