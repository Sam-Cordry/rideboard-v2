<script setup lang="ts">
import RemoveCarButton from './RemoveCarButton.vue';
import RemoveCarModal from './RemoveCarModal.vue';
import UserSelectSearch from './UserSelectSearch.vue';
</script>

<template>
  <button
    type="button"
    class="btn btn-warning mb-2"
    data-bs-toggle="modal"
    data-bs-target="#updateCarModal"
  >
    Edit Car
  </button>
  <div
    class="modal fade"
    id="updateCarModal"
    tabindex="-1"
    role="dialog"
    aria-labelledby="updateCarModalLabel"
    aria-hidden="true"
  >
    <div class="modal-dialog" role="document">
      <div class="modal-content">
        <div class="modal-header">
          <h4 class="modal-title" id="updateCarModalLabel">Edit Car</h4>
          <button type="button" class="close" data-bs-dismiss="modal" aria-label="Close">
            <span aria-hidden="true">&times;</span>
          </button>
        </div>
        <div class="modal-body">
          <div class="form-group">
            <label for="updateCarDeparture">Departure Time</label>
            <input
              v-model="departureTime"
              type="datetime-local"
              class="form-control"
              id="updateCarDeparture"
            />
          </div>
          <div class="form-group">
            <label for="updateCarReturn">Return Time</label>
            <input
              v-model="returnTime"
              :min="departureTime"
              type="datetime-local"
              class="form-control"
              id="updateCarReturn"
            />
          </div>
          <div class="form-group">
            <label for="updateCarDeparture">Maximum Capacity</label>
            <input
              v-model="maxCapacity"
              type="number"
              class="form-control"
              id="updateCarDeparture"
            />
          </div>
          <div class="form-group">
            <label for="updateCarComments">Comments</label>
            <input v-model="comment" class="form-control" id="updateCarComments" />
          </div>
          <div class="form-group">
            <label>Riders</label>
            <UserSelectSearch v-model="riders" />
          </div>
        </div>
        <RemoveCarButton />
        <div class="modal-footer">
          <button
            type="button"
            id="updateCarClose"
            class="btn btn-secondary"
            data-bs-dismiss="modal"
          >
            Close
          </button>
          <button type="button" class="btn btn-primary" @click="updateCar">Edit</button>
        </div>
      </div>
    </div>
  </div>
  <RemoveCarModal :car="car" />
</template>

<script lang="ts">
import { defineComponent, type PropType } from 'vue';
import { useEventStore } from '@/stores/events';
import { PopupType, type Car } from '@/models';
import { format } from 'date-fns';
import { usePopupStore } from '@/stores/popup';
import { useAuthStore } from '@/stores/auth';
import { validateCar } from '@/validators';

export default defineComponent({
  props: {
    car: Object as PropType<Car>
  },
  data() {
    return {
      departureTime: format(
        new Date(this.car!.departureTime.toLocaleString()),
        "yyyy-MM-dd'T'HH:mm:ss"
      ),
      returnTime: format(new Date(this.car!.returnTime.toLocaleString()), "yyyy-MM-dd'T'HH:mm:ss"),
      comment: this.car!.comment,
      maxCapacity: this.car!.maxCapacity,
      riders: this.car!.riders
    };
  },
  methods: {
    async updateCar() {
      const popupStore = usePopupStore();
      const authStore = useAuthStore();
      const eventStore = useEventStore();
      try {
        let validate = validateCar(
          authStore.user!,
          this.departureTime,
          this.returnTime,
          this.maxCapacity,
          this.riders,
          eventStore.selectedEventCars!.filter((car) => car.id !== this.car!.id)
        );
        if (validate.length != 0) {
          validate.forEach((issue) => popupStore.addPopup(PopupType.Danger, issue));
          return;
        }
        const data = {
          departureTime: new Date(this.departureTime!).toISOString(),
          returnTime: new Date(this.returnTime!).toISOString(),
          maxCapacity: this.maxCapacity,
          comment: this.comment,
          riders: this.riders.map((rider) => rider.id)
        };

        const response = await fetch(
          `/api/v1/event/${eventStore.selectedEvent!.id}/car/${this.car!.id}`,
          {
            method: 'PUT',
            headers: {
              'Content-Type': 'application/json'
            },
            body: JSON.stringify(data)
          }
        );

        if (!response.ok) {
          popupStore.addPopup(PopupType.Danger, `Failed to Edit Car (${response.status})`);
          return;
        }
        const car = eventStore.selectedEvent?.cars?.find((car) => car.id === this.car!.id);
        car!.departureTime = new Date(this.departureTime!);
        car!.returnTime = new Date(this.returnTime!);
        car!.maxCapacity = this.maxCapacity!;
        car!.comment = this.comment!;
        car!.riders = this.riders!;
        popupStore.addPopup(PopupType.Success, 'Car Updated!');
        this.closeModal();
      } catch (error) {
        console.error(error);
        popupStore.addPopup(PopupType.Danger, 'Failed to Edit Car. An unknown error occured.');
      }
    },
    closeModal() {
      const closeButton = document.getElementById('updateCarClose');
      closeButton?.click();
    }
  }
});
</script>
